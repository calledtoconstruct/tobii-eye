//! Socket server, WebSocket server, and the USB thread.
//!
//! Clients speak the existing daemon framing. The USB thread is the only
//! code that touches libusb. It posts gaze and command replies; this thread
//! is the only one that writes client sockets.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use sha1::{Digest, Sha1};

use crate::config::{self, DisplayArea};
use crate::gaze::{self, GazeSample};
use crate::ipc::{self, Message, WS_MAX_RESPONSE_PAYLOAD};
use crate::session::{Action, Feed, Session};
use crate::usb::{Usb, UsbError};

const MAX_CLIENTS: usize = 16;
const CLIENT_BUF_MAX: usize = 700 * 1024;
const WS_BUF_MAX: usize = 4096;

static QUIT: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: i32) {
    QUIT.store(true, Ordering::SeqCst);
}

pub fn socket_path() -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(runtime).join("tobiifreed/gaze.sock")
}

pub fn run(ws_bind: Option<SocketAddr>) {
    let display = config::load_display_area();
    eprintln!(
        "tobiifreed: display_area {:.0}x{:.0}mm origin=({:.0},{:.0}) z={:.0} tilt={:.2}",
        display.w_mm, display.h_mm, display.ox_mm, display.oy_mm, display.z_mm, display.tilt_deg
    );

    let path = socket_path();
    let listener = match bind_unix(&path) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("tobiifreed: failed to listen on {}: {err}", path.display());
            return;
        }
    };
    eprintln!("tobiifreed: listening on {}", path.display());

    let ws_listener = match ws_bind {
        Some(addr) => match bind_ws(addr) {
            Ok(listener) => {
                eprintln!("tobiifreed: websocket on {addr}");
                Some(listener)
            }
            Err(err) => {
                eprintln!("tobiifreed: failed to listen on {addr}: {err}");
                return;
            }
        },
        None => None,
    };

    install_signals();
    let hub = Arc::new(Hub::default());
    let usb_hub = Arc::clone(&hub);
    let usb_thread = std::thread::spawn(move || usb_main(usb_hub, display));

    let mut socks = Vec::new();
    let mut webs = Vec::new();
    while !QUIT.load(Ordering::SeqCst) {
        drain_outbound(&hub, &mut socks, &mut webs);
        accept_socks(&listener, &mut socks);
        read_socks(&hub, &mut socks);
        if let Some(ws) = ws_listener.as_ref() {
            accept_ws(ws, &mut webs);
            read_ws(&hub, &mut webs);
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    eprintln!("tobiifreed: shutting down");
    let _ = usb_thread.join();
    drop(listener);
    let _ = std::fs::remove_file(&path);
}

struct Hub {
    ready: AtomicBool,
    commands: Mutex<VecDeque<Command>>,
    outbound: Mutex<VecDeque<Outbound>>,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            ready: AtomicBool::new(false),
            commands: Mutex::new(VecDeque::new()),
            outbound: Mutex::new(VecDeque::new()),
        }
    }
}

struct Command {
    fd: i32,
    is_ws: bool,
    body: CommandBody,
}

enum CommandBody {
    GetDisplayArea,
    SetDisplayArea([f64; 5]),
    SetCorners([f64; 9]),
    AddPoint(f64, f64),
    StartCal,
    FinishCal,
    Apply(Vec<u8>),
}

enum Outbound {
    Gaze(GazeSample),
    Bytes { fd: i32, is_ws: bool, bytes: Vec<u8> },
}

struct SockClient {
    stream: UnixStream,
    subscribed: bool,
    buf: Vec<u8>,
}

struct WsClient {
    stream: TcpStream,
    open: bool,
    subscribed: bool,
    buf: Vec<u8>,
}

fn bind_unix(path: &std::path::Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

fn bind_ws(addr: SocketAddr) -> std::io::Result<TcpListener> {
    let listener = TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

fn install_signals() {
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
    }
}

fn enqueue(hub: &Hub, command: Command) {
    if !hub.ready.load(Ordering::Acquire) {
        push_bytes(
            hub,
            command.fd,
            command.is_ws,
            ipc::encode_error(command_code(&command.body), ipc::ERR_FAILED),
        );
        eprintln!(
            "tobiifreed: tracker offline, rejecting cmd {:#04x}",
            command_code(&command.body)
        );
        return;
    }
    hub.commands.lock().expect("commands").push_back(command);
}

fn command_code(body: &CommandBody) -> u8 {
    match body {
        CommandBody::GetDisplayArea => ipc::CMD_GET_DISPLAY_AREA,
        CommandBody::SetDisplayArea(_) => ipc::CMD_SET_DISPLAY_AREA,
        CommandBody::SetCorners(_) => ipc::CMD_SET_DISPLAY_AREA_CORNERS,
        CommandBody::AddPoint(_, _) => ipc::CMD_ADD_CALIBRATION_POINT,
        CommandBody::StartCal => ipc::CMD_START_CALIBRATION,
        CommandBody::FinishCal => ipc::CMD_FINISH_CALIBRATION,
        CommandBody::Apply(_) => ipc::CMD_CAL_APPLY,
    }
}

fn push_bytes(hub: &Hub, fd: i32, is_ws: bool, bytes: Vec<u8>) {
    hub.outbound
        .lock()
        .expect("outbound")
        .push_back(Outbound::Bytes { fd, is_ws, bytes });
}

fn push_gaze(hub: &Hub, sample: GazeSample) {
    let mut queue = hub.outbound.lock().expect("outbound");
    let gazes = queue.iter().filter(|item| matches!(item, Outbound::Gaze(_))).count();
    if gazes >= 64 {
        if let Some(index) = queue.iter().position(|item| matches!(item, Outbound::Gaze(_))) {
            queue.remove(index);
        }
    }
    queue.push_back(Outbound::Gaze(sample));
}

fn usb_main(hub: Arc<Hub>, display: DisplayArea) {
    let mut usb = Usb::new();
    let mut session = Session::new();
    let mut waiters: Vec<Waiter> = Vec::new();
    while !QUIT.load(Ordering::SeqCst) {
        if !hub.ready.load(Ordering::Acquire) || usb.lost() {
            hub.ready.store(false, Ordering::Release);
            if !recover(&hub, &mut usb, &mut session, &mut waiters, &display) {
                break;
            }
            continue;
        }
        if let Some(command) = hub.commands.lock().expect("commands").pop_front() {
            run_command(&hub, &mut usb, &mut session, &mut waiters, command);
            continue;
        }
        if let Some(chunk) = usb.recv(Duration::from_millis(100)) {
            take_feed(&hub, &mut session, &mut waiters, &chunk);
            for _ in 1..8 {
                let Some(more) = usb.recv(Duration::from_millis(1)) else {
                    break;
                };
                take_feed(&hub, &mut session, &mut waiters, &more);
            }
        }
    }
    usb.close_device();
}

struct Waiter {
    request_id: u32,
    fd: i32,
    is_ws: bool,
    cmd: u8,
}

fn recover(
    hub: &Hub,
    usb: &mut Usb,
    session: &mut Session,
    waiters: &mut Vec<Waiter>,
    display: &DisplayArea,
) -> bool {
    usb.close_device();
    eprintln!("tobiifreed: waiting for the eye tracker");
    while !QUIT.load(Ordering::SeqCst) {
        match usb.open() {
            Ok(()) => {
                if bring_up(hub, usb, session, waiters, display) {
                    hub.ready.store(true, Ordering::Release);
                    eprintln!("tobiifreed: tracker ready");
                    return true;
                }
                usb.close_device();
            }
            Err(UsbError::NotFound) => {}
            Err(err) => eprintln!("tobiifreed: failed to open USB: {err}"),
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

impl std::fmt::Display for UsbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsbError::Init(err) | UsbError::Claim(err) | UsbError::Session(err) => write!(f, "{err}"),
            UsbError::NotFound => write!(f, "device 2104:0313 not found"),
        }
    }
}

fn bring_up(
    hub: &Hub,
    usb: &mut Usb,
    session: &mut Session,
    waiters: &mut Vec<Waiter>,
    display: &DisplayArea,
) -> bool {
    waiters.clear();
    session.handshake_init(0x500);
    if !drive(hub, usb, session, Duration::from_secs(150), Session::handshake_poll) {
        eprintln!("tobiifreed: handshake failed");
        return false;
    }
    match query_display(hub, usb, session) {
        Some(corners) if plane_is_reset(corners) => {
            eprintln!("tobiifreed: device display area looks reset, applying config");
            let (tl, tr, bl) = display.corners();
            let values = [
                tl[0], tl[1], tl[2], tr[0], tr[1], tr[2], bl[0], bl[1], bl[2],
            ];
            let _ = session.request_set_display_corners(values);
            let bytes = session.out().to_vec();
            if !bytes.is_empty() && !usb.send(&bytes) {
                eprintln!("tobiifreed: failed to set display area from config");
            }
        }
        Some(corners) => {
            eprintln!(
                "tobiifreed: device display TL=({:.0},{:.0},{:.0}) TR=({:.0},{:.0},{:.0}) BL=({:.0},{:.0},{:.0})",
                corners[0][0], corners[0][1], corners[0][2],
                corners[1][0], corners[1][1], corners[1][2],
                corners[2][0], corners[2][1], corners[2][2],
            );
        }
        None => eprintln!("tobiifreed: display area read failed"),
    }
    true
}

fn plane_is_reset(corners: [[f64; 3]; 3]) -> bool {
    let width = (corners[1][0] - corners[0][0]).abs();
    let height = (corners[0][1] - corners[2][1]).abs();
    width < 50.0 || height < 50.0
}

fn query_display(hub: &Hub, usb: &mut Usb, session: &mut Session) -> Option<[[f64; 3]; 3]> {
    session.begin_request();
    let _ = session.request_get_display_area();
    let bytes = session.out().to_vec();
    if bytes.is_empty() || !usb.send(&bytes) {
        session.end_request();
        return None;
    }
    let payload = wait_held(hub, usb, session, 20);
    session.end_request();
    payload.as_deref().and_then(gaze::decode_display_area)
}

fn run_command(
    hub: &Hub,
    usb: &mut Usb,
    session: &mut Session,
    waiters: &mut Vec<Waiter>,
    command: Command,
) {
    if usb.lost() || !hub.ready.load(Ordering::Acquire) {
        reply(hub, command.fd, command.is_ws, command_code(&command.body), false, &[]);
        return;
    }
    let cmd = command_code(&command.body);
    match command.body {
        CommandBody::GetDisplayArea => {
            let request_id = session.request_get_display_area();
            send_tracked(usb, waiters, command.fd, command.is_ws, cmd, request_id, session);
        }
        CommandBody::SetDisplayArea(values) => {
            let _ = session.request_set_display_area(values);
            let bytes = session.out().to_vec();
            if !bytes.is_empty() {
                let _ = usb.send(&bytes);
            }
        }
        CommandBody::SetCorners(values) => {
            let _ = session.request_set_display_corners(values);
            let bytes = session.out().to_vec();
            if !bytes.is_empty() {
                let _ = usb.send(&bytes);
            }
        }
        CommandBody::AddPoint(x, y) => {
            let request_id = session.request_add_point(x, y);
            send_tracked(usb, waiters, command.fd, command.is_ws, cmd, request_id, session);
        }
        CommandBody::StartCal => {
            let ok = start_calibration(hub, usb, session);
            reply(hub, command.fd, command.is_ws, cmd, ok, &[]);
        }
        CommandBody::FinishCal => match finish_calibration(hub, usb, session) {
            Some(blob) => reply(hub, command.fd, command.is_ws, cmd, true, &blob),
            None => reply(hub, command.fd, command.is_ws, cmd, false, &[]),
        },
        CommandBody::Apply(blob) => {
            if blob.is_empty() {
                reply(hub, command.fd, command.is_ws, cmd, false, &[]);
                return;
            }
            session.cal_apply_init(&blob);
            let ok = drive(hub, usb, session, Duration::from_secs(120), Session::cal_apply_poll);
            reply(hub, command.fd, command.is_ws, cmd, ok, &[]);
        }
    }
}

fn send_tracked(
    usb: &mut Usb,
    waiters: &mut Vec<Waiter>,
    fd: i32,
    is_ws: bool,
    cmd: u8,
    request_id: u32,
    session: &Session,
) {
    let bytes = session.out().to_vec();
    if bytes.is_empty() || !usb.send(&bytes) {
        eprintln!("tobiifreed: USB send failed for cmd {cmd:#04x}");
        return;
    }
    if request_id != 0 {
        waiters.push(Waiter {
            request_id,
            fd,
            is_ws,
            cmd,
        });
    }
}

fn start_calibration(hub: &Hub, usb: &mut Usb, session: &mut Session) -> bool {
    session.cal_start_init();
    if !drive(hub, usb, session, Duration::from_secs(150), Session::cal_start_poll) {
        return false;
    }
    if !send_and_await(hub, usb, session, Session::request_cal_start) {
        return false;
    }
    let _ = send_and_await(hub, usb, session, Session::request_cal_clear);
    true
}

fn finish_calibration(hub: &Hub, usb: &mut Usb, session: &mut Session) -> Option<Vec<u8>> {
    session.cal_finish_init();
    if !drive(hub, usb, session, Duration::from_secs(150), Session::cal_finish_poll) {
        return None;
    }
    Some(session.cal_finish_blob().to_vec())
}

fn send_and_await(
    hub: &Hub,
    usb: &mut Usb,
    session: &mut Session,
    build: fn(&mut Session) -> u32,
) -> bool {
    session.begin_request();
    let _ = build(session);
    let bytes = session.out().to_vec();
    if bytes.is_empty() || !usb.send(&bytes) {
        session.end_request();
        return false;
    }
    let got = wait_held(hub, usb, session, 30).is_some();
    session.end_request();
    got
}

fn drive(
    hub: &Hub,
    usb: &mut Usb,
    session: &mut Session,
    timeout: Duration,
    poll: fn(&mut Session) -> Action,
) -> bool {
    let start = Instant::now();
    loop {
        if QUIT.load(Ordering::SeqCst) || start.elapsed() > timeout {
            return false;
        }
        match poll(session) {
            Action::Send => {
                let bytes = session.out().to_vec();
                if !bytes.is_empty() && !usb.send(&bytes) {
                    return false;
                }
                drain_usb(hub, usb, session, 10);
            }
            Action::Recv => {
                drain_usb(hub, usb, session, 5);
            }
            Action::Done => return true,
            Action::Err => return false,
        }
    }
}

fn drain_usb(hub: &Hub, usb: &mut Usb, session: &mut Session, max_reads: u32) {
    if let Some(chunk) = usb.recv(Duration::from_millis(100)) {
        take_feed_gaze_only(hub, session, &chunk);
    } else {
        return;
    }
    for _ in 1..max_reads {
        let Some(chunk) = usb.recv(Duration::from_millis(1)) else {
            break;
        };
        take_feed_gaze_only(hub, session, &chunk);
    }
}

fn wait_held(hub: &Hub, usb: &mut Usb, session: &mut Session, max_reads: u32) -> Option<Vec<u8>> {
    for _ in 0..max_reads {
        let Some(chunk) = usb.recv(Duration::from_millis(100)) else {
            continue;
        };
        take_feed_gaze_only(hub, session, &chunk);
        if let Some(payload) = session.take_held() {
            return Some(payload);
        }
        for _ in 1..8 {
            let Some(more) = usb.recv(Duration::from_millis(1)) else {
                break;
            };
            take_feed_gaze_only(hub, session, &more);
            if let Some(payload) = session.take_held() {
                return Some(payload);
            }
        }
    }
    None
}

fn take_feed(hub: &Hub, session: &mut Session, waiters: &mut Vec<Waiter>, chunk: &[u8]) {
    for event in session.feed(chunk) {
        match event {
            Feed::Gaze(sample) => push_gaze(hub, sample),
            Feed::Error(code) => eprintln!("tobiifreed: usb parse error {code}"),
            Feed::Response { request_id, payload } => {
                let Some(index) = waiters.iter().position(|waiter| waiter.request_id == request_id) else {
                    continue;
                };
                let waiter = waiters.remove(index);
                reply(hub, waiter.fd, waiter.is_ws, waiter.cmd, true, &payload);
            }
        }
    }
}

fn take_feed_gaze_only(hub: &Hub, session: &mut Session, chunk: &[u8]) {
    for event in session.feed(chunk) {
        if let Feed::Gaze(sample) = event {
            push_gaze(hub, sample);
        }
    }
}

fn reply(hub: &Hub, fd: i32, is_ws: bool, cmd: u8, ok: bool, payload: &[u8]) {
    let bytes = if !ok {
        ipc::encode_error(cmd, ipc::ERR_FAILED)
    } else if is_ws && payload.len() > WS_MAX_RESPONSE_PAYLOAD {
        eprintln!("tobiifreed: websocket payload is {} bytes, sending error", payload.len());
        ipc::encode_error(cmd, ipc::ERR_TOO_LARGE)
    } else {
        ipc::encode_response(cmd, payload)
    };
    push_bytes(hub, fd, is_ws, bytes);
}

fn drain_outbound(hub: &Hub, socks: &mut Vec<SockClient>, webs: &mut Vec<WsClient>) {
    let pending: Vec<_> = hub.outbound.lock().expect("outbound").drain(..).collect();
    for event in pending {
        match event {
            Outbound::Gaze(sample) => {
                let msg = ipc::encode_gaze(sample);
                socks.retain(|client| !client.subscribed || write_fd(client.stream.as_raw_fd(), &msg));
                let frame = ws_frame(&msg);
                webs.retain(|client| {
                    !client.open || !client.subscribed || write_fd(client.stream.as_raw_fd(), &frame)
                });
            }
            Outbound::Bytes { fd, is_ws, bytes } => {
                if is_ws {
                    let frame = ws_frame(&bytes);
                    if let Some(client) = webs.iter().find(|client| client.stream.as_raw_fd() == fd) {
                        let _ = write_fd(client.stream.as_raw_fd(), &frame);
                    }
                } else if let Some(client) = socks.iter().find(|client| client.stream.as_raw_fd() == fd) {
                    let _ = write_fd(client.stream.as_raw_fd(), &bytes);
                }
            }
        }
    }
}

fn accept_socks(listener: &UnixListener, clients: &mut Vec<SockClient>) {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if clients.len() >= MAX_CLIENTS {
                    drop(stream);
                    continue;
                }
                if stream.set_nonblocking(true).is_err() {
                    continue;
                }
                clients.push(SockClient {
                    stream,
                    subscribed: false,
                    buf: Vec::new(),
                });
                eprintln!("tobiifreed: client connected (total: {})", clients.len());
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => break,
            Err(_) => break,
        }
    }
}

fn read_socks(hub: &Hub, clients: &mut Vec<SockClient>) {
    let mut dead = Vec::new();
    for (index, client) in clients.iter_mut().enumerate() {
        if !read_into(&mut client.stream, &mut client.buf, CLIENT_BUF_MAX) {
            dead.push(index);
            continue;
        }
        loop {
            match ipc::pop_message(&mut client.buf) {
                Ok(None) => break,
                Ok(Some(message)) => {
                    if message.kind == ipc::CMD_SUBSCRIBE {
                        client.subscribed = true;
                        eprintln!("tobiifreed: client subscribed to gaze");
                    } else {
                        dispatch_sock(hub, client, message);
                    }
                }
                Err(()) => {
                    dead.push(index);
                    break;
                }
            }
        }
    }
    for index in dead.into_iter().rev() {
        clients.remove(index);
        eprintln!("tobiifreed: client disconnected (total: {})", clients.len());
    }
}

fn dispatch_sock(hub: &Hub, client: &SockClient, message: Message) {
    let fd = client.stream.as_raw_fd();
    match message.kind {
        ipc::CMD_SUBSCRIBE => {}
        ipc::CMD_DISCONNECT => {}
        ipc::CMD_GET_DISPLAY_AREA => enqueue(hub, Command { fd, is_ws: false, body: CommandBody::GetDisplayArea }),
        ipc::CMD_SET_DISPLAY_AREA => {
            if let Some(values) = ipc::read_f64s::<5>(&message.payload) {
                enqueue(hub, Command { fd, is_ws: false, body: CommandBody::SetDisplayArea(values) });
            }
        }
        ipc::CMD_SET_DISPLAY_AREA_CORNERS => {
            if let Some(values) = ipc::read_f64s::<9>(&message.payload) {
                enqueue(hub, Command { fd, is_ws: false, body: CommandBody::SetCorners(values) });
            }
        }
        ipc::CMD_ADD_CALIBRATION_POINT => {
            if let Some([x, y]) = ipc::read_f64s::<2>(&message.payload) {
                enqueue(hub, Command { fd, is_ws: false, body: CommandBody::AddPoint(x, y) });
            }
        }
        ipc::CMD_START_CALIBRATION => enqueue(hub, Command { fd, is_ws: false, body: CommandBody::StartCal }),
        ipc::CMD_FINISH_CALIBRATION => enqueue(hub, Command { fd, is_ws: false, body: CommandBody::FinishCal }),
        ipc::CMD_CAL_APPLY => enqueue(hub, Command { fd, is_ws: false, body: CommandBody::Apply(message.payload) }),
        _ => eprintln!("tobiifreed: unknown cmd {:#04x}", message.kind),
    }
}

fn accept_ws(listener: &TcpListener, clients: &mut Vec<WsClient>) {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if clients.len() >= MAX_CLIENTS {
                    drop(stream);
                    continue;
                }
                let _ = stream.set_nodelay(true);
                if stream.set_nonblocking(true).is_err() {
                    continue;
                }
                clients.push(WsClient {
                    stream,
                    open: false,
                    subscribed: false,
                    buf: Vec::new(),
                });
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => break,
            Err(_) => break,
        }
    }
}

fn read_ws(hub: &Hub, clients: &mut Vec<WsClient>) {
    let mut dead = Vec::new();
    for (index, client) in clients.iter_mut().enumerate() {
        if !read_into(&mut client.stream, &mut client.buf, WS_BUF_MAX) {
            dead.push(index);
            continue;
        }
        if !client.open {
            match try_upgrade(client) {
                Upgrade::Pending => {}
                Upgrade::Failed => dead.push(index),
                Upgrade::Open => {}
            }
        }
        if client.open {
            match take_ws_frames(hub, client) {
                Ok(()) => {}
                Err(()) => dead.push(index),
            }
        }
    }
    for index in dead.into_iter().rev() {
        clients.remove(index);
    }
}

enum Upgrade {
    Pending,
    Open,
    Failed,
}

fn try_upgrade(client: &mut WsClient) -> Upgrade {
    let Some(end) = find_header_end(&client.buf) else {
        return Upgrade::Pending;
    };
    let Some(key) = extract_ws_key(&client.buf[..end]) else {
        return Upgrade::Failed;
    };
    let accept = ws_accept(key);
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    if !write_fd(client.stream.as_raw_fd(), response.as_bytes()) {
        return Upgrade::Failed;
    }
    client.buf.drain(..end);
    client.open = true;
    Upgrade::Open
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|window| window == b"\r\n\r\n").map(|index| index + 4)
}

fn extract_ws_key(headers: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(headers).ok()?;
    for line in text.split("\r\n") {
        let Some(rest) = line.strip_prefix("Sec-WebSocket-Key:")
            .or_else(|| line.strip_prefix("sec-websocket-key:"))
        else {
            continue;
        };
        let rest = rest.trim();
        if !rest.is_empty() && rest.len() <= 64 {
            return Some(rest);
        }
    }
    None
}

pub fn ws_accept(key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

fn take_ws_frames(hub: &Hub, client: &mut WsClient) -> Result<(), ()> {
    let mut pos = 0;
    while pos < client.buf.len() {
        let remaining = client.buf.len() - pos;
        if remaining < 2 {
            break;
        }
        let b0 = client.buf[pos];
        let b1 = client.buf[pos + 1];
        let opcode = b0 & 0x0f;
        let masked = b1 & 0x80 != 0;
        let mut payload_len = (b1 & 0x7f) as usize;
        let mut header_len = 2;
        if payload_len == 126 {
            if remaining < 4 {
                break;
            }
            payload_len = u16::from_be_bytes(client.buf[pos + 2..pos + 4].try_into().unwrap()) as usize;
            header_len = 4;
        } else if payload_len == 127 {
            if remaining < 10 {
                break;
            }
            let len = u64::from_be_bytes(client.buf[pos + 2..pos + 10].try_into().unwrap());
            if len > 65536 {
                return Err(());
            }
            payload_len = len as usize;
            header_len = 10;
        }
        let mask_len = if masked { 4 } else { 0 };
        let total = header_len + mask_len + payload_len;
        if remaining < total {
            break;
        }
        let mask = if masked {
            Some(client.buf[pos + header_len..pos + header_len + 4].to_vec())
        } else {
            None
        };
        let mut payload = client.buf[pos + header_len + mask_len..pos + total].to_vec();
        if let Some(mask) = &mask {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
        }
        if opcode == 0x8 {
            let _ = write_fd(client.stream.as_raw_fd(), &[0x88, 0x00]);
            return Err(());
        } else if opcode == 0x9 {
            let mut pong = Vec::with_capacity(2 + payload.len().min(125));
            pong.push(0x8a);
            let len = payload.len().min(125);
            pong.push(len as u8);
            pong.extend_from_slice(&payload[..len]);
            let _ = write_fd(client.stream.as_raw_fd(), &pong);
        } else if opcode == 0x2 {
            if let Ok(Some(message)) = ipc::pop_message(&mut payload) {
                if message.kind == ipc::CMD_SUBSCRIBE {
                    client.subscribed = true;
                    let warp = ipc::encode_screen_warp(config::load_screen_warp());
                    push_bytes(hub, client.stream.as_raw_fd(), true, warp);
                    eprintln!("tobiifreed: ws client subscribed to gaze");
                } else {
                    dispatch_ws(hub, client, message);
                }
            }
        }
        pos += total;
    }
    if pos > 0 {
        client.buf.drain(..pos);
    }
    Ok(())
}

fn dispatch_ws(hub: &Hub, client: &WsClient, message: Message) {
    let fd = client.stream.as_raw_fd();
    if message.kind == ipc::CMD_DISCONNECT {
        return;
    }
    let body = match message.kind {
        ipc::CMD_GET_DISPLAY_AREA => CommandBody::GetDisplayArea,
        ipc::CMD_SET_DISPLAY_AREA => match ipc::read_f64s(&message.payload) {
            Some(values) => CommandBody::SetDisplayArea(values),
            None => return,
        },
        ipc::CMD_SET_DISPLAY_AREA_CORNERS => match ipc::read_f64s(&message.payload) {
            Some(values) => CommandBody::SetCorners(values),
            None => return,
        },
        ipc::CMD_ADD_CALIBRATION_POINT => match ipc::read_f64s::<2>(&message.payload) {
            Some([x, y]) => CommandBody::AddPoint(x, y),
            None => return,
        },
        ipc::CMD_START_CALIBRATION => CommandBody::StartCal,
        ipc::CMD_FINISH_CALIBRATION => CommandBody::FinishCal,
        ipc::CMD_CAL_APPLY => CommandBody::Apply(message.payload),
        _ => return,
    };
    enqueue(hub, Command { fd, is_ws: true, body });
}

fn ws_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + payload.len());
    out.push(0x82);
    if payload.len() < 126 {
        out.push(payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        out.push(126);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    out.extend_from_slice(payload);
    out
}

fn read_into(stream: &mut impl Read, buf: &mut Vec<u8>, max: usize) -> bool {
    if buf.len() >= max {
        return false;
    }
    let mut tmp = [0u8; 8192];
    match stream.read(&mut tmp) {
        Ok(0) => false,
        Ok(n) => {
            let room = max - buf.len();
            buf.extend_from_slice(&tmp[..n.min(room)]);
            n <= room
        }
        Err(err) if err.kind() == ErrorKind::WouldBlock => true,
        Err(_) => false,
    }
}

fn write_fd(fd: i32, mut data: &[u8]) -> bool {
    while !data.is_empty() {
        let n = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
        if n > 0 {
            data = &data[n as usize..];
            continue;
        }
        let err = std::io::Error::last_os_error();
        if err.kind() != ErrorKind::WouldBlock {
            return false;
        }
        let mut pollfd = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut pollfd, 1, 5000) };
        if ready <= 0 {
            return false;
        }
        let bad = libc::POLLERR | libc::POLLHUP | libc::POLLNVAL;
        if pollfd.revents & bad != 0 {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_accept_matches_rfc6455() {
        assert_eq!(ws_accept("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }
}
