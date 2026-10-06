//! Tracker session: sequence numbers, the handshake, and calibration.
//!
//! The USB thread sends `out` when the poll returns [`Action::Send`], then
//! feeds received chunks back in. Responses that belong to a client command
//! come out of [`Session::feed`] as [`Feed::Response`]. Responses that belong
//! to the handshake or a calibration step stay inside the session.

use crate::frame::{self, MAGIC_RSP, REALM_KEY};
use crate::gaze::GazeSample;
use crate::parser::{Inbound, Parser};

const CAL_BLOB_MAX: usize = 640 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Send,
    Recv,
    Done,
    Err,
}

#[derive(Clone, Debug)]
pub enum Feed {
    Gaze(GazeSample),
    Response { request_id: u32, payload: Vec<u8> },
    Error(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handshake {
    BuildHello,
    AwaitHello,
    BuildQueryRealm,
    AwaitQueryRealm,
    BuildOpenRealm,
    AwaitOpenRealm,
    BuildRealmAuth,
    AwaitRealmAuth,
    BuildSubscribe,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CalStart {
    Idle,
    BuildQueryRealm,
    AwaitQueryRealm,
    BuildOpenRealm,
    AwaitOpenRealm,
    BuildRealmAuth,
    AwaitRealmAuth,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CalFinish {
    Idle,
    BuildPointsApply,
    AwaitPointsApply,
    BuildStop,
    AwaitStop,
    BuildRetrieve,
    AwaitRetrieve,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CalApply {
    Idle,
    RealmUnlock,
    BuildApply,
    AwaitApply,
    BuildCloseRealm,
    AwaitCloseRealm,
    Done,
    Failed,
}

pub struct Session {
    next_seq: u32,
    next_req: u32,
    pending: Vec<(u32, u32)>,
    out: Vec<u8>,
    parser: Parser,
    holding: bool,
    resp_ready: bool,
    held: Vec<u8>,
    stream_id: u16,
    handshake: Handshake,
    realm_type: u32,
    realm_id: u32,
    field_210: u32,
    cal_start: CalStart,
    cal_finish: CalFinish,
    cal_apply: CalApply,
    blob: Vec<u8>,
    truncated: bool,
}

impl Session {
    pub fn new() -> Self {
        Self {
            next_seq: 1,
            next_req: 1,
            pending: Vec::new(),
            out: Vec::new(),
            parser: Parser::new(),
            holding: false,
            resp_ready: false,
            held: Vec::new(),
            stream_id: 0x500,
            handshake: Handshake::Done,
            realm_type: 0,
            realm_id: 0,
            field_210: 0,
            cal_start: CalStart::Idle,
            cal_finish: CalFinish::Idle,
            cal_apply: CalApply::Idle,
            blob: Vec::new(),
            truncated: false,
        }
    }

    pub fn out(&self) -> &[u8] {
        &self.out
    }

    pub fn reset(&mut self) {
        self.next_seq = 1;
        self.next_req = 1;
        self.pending.clear();
        self.out.clear();
        self.parser.reset();
        self.holding = false;
        self.resp_ready = false;
        self.held.clear();
        self.truncated = false;
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Feed> {
        let mut events = Vec::new();
        for inbound in self.parser.feed(chunk) {
            match inbound {
                Inbound::Gaze(sample) => events.push(Feed::Gaze(sample)),
                Inbound::Error(code) => events.push(Feed::Error(code)),
                Inbound::Frame(frame) => {
                    if frame.magic != MAGIC_RSP {
                        continue;
                    }
                    let Some(request_id) = self.take_pending(frame.seq) else {
                        continue;
                    };
                    if self.holding {
                        if frame.payload.len() > CAL_BLOB_MAX {
                            self.truncated = true;
                        }
                        let n = frame.payload.len().min(CAL_BLOB_MAX);
                        self.held.clear();
                        self.held.extend_from_slice(&frame.payload[..n]);
                        self.resp_ready = true;
                    } else {
                        events.push(Feed::Response {
                            request_id,
                            payload: frame.payload,
                        });
                    }
                }
            }
        }
        events
    }

    pub fn handshake_init(&mut self, stream_id: u16) {
        self.reset();
        self.stream_id = stream_id;
        self.handshake = Handshake::BuildHello;
        self.holding = true;
        self.resp_ready = false;
        self.realm_type = 0;
        self.realm_id = 0;
        self.field_210 = 0;
    }

    pub fn handshake_poll(&mut self) -> Action {
        loop {
            match self.handshake {
                Handshake::BuildHello => {
                    self.queue_tracked(frame::hello(self.peek_seq()));
                    self.resp_ready = false;
                    self.handshake = Handshake::AwaitHello;
                    return Action::Send;
                }
                Handshake::AwaitHello => {
                    if self.resp_ready {
                        self.handshake = Handshake::BuildQueryRealm;
                        continue;
                    }
                    return Action::Recv;
                }
                Handshake::BuildQueryRealm => {
                    self.queue_tracked(frame::query_realm(self.peek_seq()));
                    self.resp_ready = false;
                    self.handshake = Handshake::AwaitQueryRealm;
                    return Action::Send;
                }
                Handshake::AwaitQueryRealm => {
                    if self.resp_ready {
                        self.realm_type = if self.held.len() >= 6 {
                            frame::realm_first_u32(&self.held)
                        } else {
                            0
                        };
                        self.handshake = Handshake::BuildOpenRealm;
                        continue;
                    }
                    return Action::Recv;
                }
                Handshake::BuildOpenRealm => {
                    self.queue_tracked(frame::open_realm(self.peek_seq(), self.realm_type));
                    self.resp_ready = false;
                    self.handshake = Handshake::AwaitOpenRealm;
                    return Action::Send;
                }
                Handshake::AwaitOpenRealm => {
                    if self.resp_ready {
                        if self.realm_type == 0 {
                            self.handshake = Handshake::BuildSubscribe;
                            continue;
                        }
                        if self.held.len() < 12 {
                            self.handshake = Handshake::Failed;
                            continue;
                        }
                        self.realm_id = frame::realm_u32_at(&self.held, 0);
                        self.field_210 = frame::realm_u32_at(&self.held, 1);
                        self.handshake = Handshake::BuildRealmAuth;
                        continue;
                    }
                    return Action::Recv;
                }
                Handshake::BuildRealmAuth => {
                    let Some(challenge) = frame::realm_challenge(&self.held) else {
                        self.handshake = Handshake::Failed;
                        continue;
                    };
                    let digest = frame::hmac_md5(REALM_KEY, challenge);
                    self.queue_tracked(frame::realm_response(
                        self.peek_seq(),
                        self.realm_id,
                        self.field_210,
                        &digest,
                    ));
                    self.resp_ready = false;
                    self.handshake = Handshake::AwaitRealmAuth;
                    return Action::Send;
                }
                Handshake::AwaitRealmAuth => {
                    if self.resp_ready {
                        self.handshake = Handshake::BuildSubscribe;
                        continue;
                    }
                    return Action::Recv;
                }
                Handshake::BuildSubscribe => {
                    let seq = self.alloc_seq();
                    self.out = frame::subscribe(seq, self.stream_id);
                    self.handshake = Handshake::Done;
                    return Action::Send;
                }
                Handshake::Done => {
                    self.holding = false;
                    return Action::Done;
                }
                Handshake::Failed => {
                    self.holding = false;
                    return Action::Err;
                }
            }
        }
    }

    /// Ask the device for its display plane. The caller sends `out` and feeds
    /// until [`Session::take_held`] returns the reply.
    pub fn begin_request(&mut self) {
        self.holding = true;
        self.resp_ready = false;
        self.held.clear();
    }

    pub fn end_request(&mut self) {
        self.holding = false;
    }

    pub fn take_held(&mut self) -> Option<Vec<u8>> {
        if !self.resp_ready {
            return None;
        }
        self.resp_ready = false;
        Some(std::mem::take(&mut self.held))
    }

    pub fn request_get_display_area(&mut self) -> u32 {
        self.queue_tracked(frame::get_display_area(self.peek_seq()))
    }

    pub fn request_set_display_area(&mut self, values: [f64; 5]) -> u32 {
        let seq = self.alloc_seq();
        self.out = frame::set_display_area(seq, values[0], values[1], values[2], values[3], values[4]);
        0
    }

    pub fn request_set_display_corners(&mut self, values: [f64; 9]) -> u32 {
        let seq = self.alloc_seq();
        self.out = frame::set_display_area_corners(
            seq,
            [values[0], values[1], values[2]],
            [values[3], values[4], values[5]],
            [values[6], values[7], values[8]],
        );
        0
    }

    /// Both eyes. The daemon protocol's add-point command does not carry a mask.
    pub fn request_add_point(&mut self, x: f64, y: f64) -> u32 {
        self.queue_tracked(frame::cal_point_add2d(self.peek_seq(), x, y, 3))
    }

    pub fn request_cal_start(&mut self) -> u32 {
        self.queue_tracked(frame::cal_start(self.peek_seq()))
    }

    pub fn request_cal_clear(&mut self) -> u32 {
        self.queue_tracked(frame::cal_clear(self.peek_seq()))
    }

    pub fn cal_start_init(&mut self) {
        self.cal_start = CalStart::BuildQueryRealm;
        self.realm_type = 0;
        self.realm_id = 0;
        self.field_210 = 0;
        self.holding = true;
        self.resp_ready = false;
        self.held.clear();
    }

    pub fn cal_start_poll(&mut self) -> Action {
        loop {
            match self.cal_start {
                CalStart::Idle | CalStart::Done => {
                    self.holding = false;
                    return Action::Done;
                }
                CalStart::Failed => {
                    self.holding = false;
                    return Action::Err;
                }
                CalStart::BuildQueryRealm => {
                    self.queue_tracked(frame::query_realm(self.peek_seq()));
                    self.resp_ready = false;
                    self.cal_start = CalStart::AwaitQueryRealm;
                    return Action::Send;
                }
                CalStart::AwaitQueryRealm => {
                    if self.resp_ready {
                        self.realm_type = if self.held.len() >= 6 {
                            frame::realm_first_u32(&self.held)
                        } else {
                            0
                        };
                        self.cal_start = CalStart::BuildOpenRealm;
                        continue;
                    }
                    return Action::Recv;
                }
                CalStart::BuildOpenRealm => {
                    self.queue_tracked(frame::open_realm(self.peek_seq(), self.realm_type));
                    self.resp_ready = false;
                    self.cal_start = CalStart::AwaitOpenRealm;
                    return Action::Send;
                }
                CalStart::AwaitOpenRealm => {
                    if self.resp_ready {
                        if self.realm_type == 0 {
                            self.realm_id = if self.held.len() >= 6 {
                                frame::realm_first_u32(&self.held)
                            } else {
                                0
                            };
                            self.cal_start = CalStart::Done;
                            continue;
                        }
                        if self.held.len() < 12 {
                            self.cal_start = CalStart::Failed;
                            continue;
                        }
                        self.realm_id = frame::realm_u32_at(&self.held, 0);
                        self.field_210 = frame::realm_u32_at(&self.held, 1);
                        self.cal_start = CalStart::BuildRealmAuth;
                        continue;
                    }
                    return Action::Recv;
                }
                CalStart::BuildRealmAuth => {
                    let Some(challenge) = frame::realm_challenge(&self.held) else {
                        self.cal_start = CalStart::Failed;
                        continue;
                    };
                    let digest = frame::hmac_md5(REALM_KEY, challenge);
                    self.queue_tracked(frame::realm_response(
                        self.peek_seq(),
                        self.realm_id,
                        self.field_210,
                        &digest,
                    ));
                    self.resp_ready = false;
                    self.cal_start = CalStart::AwaitRealmAuth;
                    return Action::Send;
                }
                CalStart::AwaitRealmAuth => {
                    if self.resp_ready {
                        self.cal_start = CalStart::Done;
                        continue;
                    }
                    return Action::Recv;
                }
            }
        }
    }

    pub fn cal_finish_init(&mut self) {
        self.cal_finish = CalFinish::BuildPointsApply;
        self.blob.clear();
        self.truncated = false;
        self.holding = true;
        self.resp_ready = false;
        self.held.clear();
    }

    pub fn cal_finish_blob(&self) -> &[u8] {
        &self.blob
    }

    pub fn cal_finish_poll(&mut self) -> Action {
        loop {
            match self.cal_finish {
                CalFinish::Idle | CalFinish::Done => {
                    self.holding = false;
                    return Action::Done;
                }
                CalFinish::Failed => {
                    self.holding = false;
                    return Action::Err;
                }
                CalFinish::BuildPointsApply => {
                    self.queue_tracked(frame::cal_points_apply(self.peek_seq()));
                    self.resp_ready = false;
                    self.cal_finish = CalFinish::AwaitPointsApply;
                    return Action::Send;
                }
                CalFinish::AwaitPointsApply => {
                    if self.resp_ready {
                        self.cal_finish = CalFinish::BuildStop;
                        continue;
                    }
                    return Action::Recv;
                }
                CalFinish::BuildStop => {
                    self.queue_tracked(frame::cal_stop(self.peek_seq()));
                    self.resp_ready = false;
                    self.cal_finish = CalFinish::AwaitStop;
                    return Action::Send;
                }
                CalFinish::AwaitStop => {
                    if self.resp_ready {
                        self.cal_finish = CalFinish::BuildRetrieve;
                        continue;
                    }
                    return Action::Recv;
                }
                CalFinish::BuildRetrieve => {
                    self.queue_tracked(frame::cal_retrieve(self.peek_seq()));
                    self.resp_ready = false;
                    self.cal_finish = CalFinish::AwaitRetrieve;
                    return Action::Send;
                }
                CalFinish::AwaitRetrieve => {
                    if self.resp_ready {
                        if self.truncated {
                            self.cal_finish = CalFinish::Failed;
                            continue;
                        }
                        let raw = if self.held.len() >= 2 {
                            self.held[2..].to_vec()
                        } else {
                            Vec::new()
                        };
                        self.blob = raw;
                        self.cal_finish = CalFinish::Done;
                        continue;
                    }
                    return Action::Recv;
                }
            }
        }
    }

    pub fn cal_apply_init(&mut self, blob: &[u8]) {
        self.blob = blob.to_vec();
        self.cal_apply = CalApply::RealmUnlock;
        self.cal_start_init();
    }

    pub fn cal_apply_poll(&mut self) -> Action {
        loop {
            match self.cal_apply {
                CalApply::Idle | CalApply::Done => {
                    self.holding = false;
                    return Action::Done;
                }
                CalApply::Failed => {
                    self.holding = false;
                    return Action::Err;
                }
                CalApply::RealmUnlock => match self.cal_start_poll() {
                    Action::Done => {
                        self.cal_apply = CalApply::BuildApply;
                        self.holding = true;
                        self.resp_ready = false;
                        continue;
                    }
                    Action::Err => {
                        self.cal_apply = CalApply::Failed;
                        continue;
                    }
                    other => return other,
                },
                CalApply::BuildApply => {
                    let blob = self.blob.clone();
                    let seq = self.peek_seq();
                    let built = frame::cal_apply(seq, &blob);
                    self.queue_tracked(built);
                    self.resp_ready = false;
                    self.cal_apply = CalApply::AwaitApply;
                    return Action::Send;
                }
                CalApply::AwaitApply => {
                    if self.resp_ready {
                        self.cal_apply = CalApply::BuildCloseRealm;
                        continue;
                    }
                    return Action::Recv;
                }
                CalApply::BuildCloseRealm => {
                    self.queue_tracked(frame::close_realm(self.peek_seq(), self.realm_id));
                    self.resp_ready = false;
                    self.cal_apply = CalApply::AwaitCloseRealm;
                    return Action::Send;
                }
                CalApply::AwaitCloseRealm => {
                    if self.resp_ready {
                        self.cal_apply = CalApply::Done;
                        continue;
                    }
                    return Action::Recv;
                }
            }
        }
    }

    fn peek_seq(&self) -> u32 {
        self.next_seq
    }

    fn alloc_seq(&mut self) -> u32 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        if self.next_seq == 0 {
            self.next_seq = 1;
        }
        seq
    }

    fn alloc_req(&mut self) -> u32 {
        let id = self.next_req;
        self.next_req = self.next_req.wrapping_add(1);
        if self.next_req == 0 {
            self.next_req = 1;
        }
        id
    }

    /// Build `frame` for the next sequence number and remember it as a request.
    fn queue_tracked(&mut self, frame_bytes: Vec<u8>) -> u32 {
        // `frame_bytes` was built with peek_seq(), which this consumes.
        let seq = self.alloc_seq();
        let req = self.alloc_req();
        self.pending.push((seq, req));
        if self.pending.len() > 32 {
            self.pending.remove(0);
        }
        self.out = frame_bytes;
        req
    }

    fn take_pending(&mut self, seq: u32) -> Option<u32> {
        let index = self.pending.iter().position(|(pending, _)| *pending == seq)?;
        Some(self.pending.remove(index).1)
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::MAGIC_RSP;
    use crate::gaze::decode_display_area;

    #[test]
    fn handshake_sends_hello_then_advances_when_the_device_replies() {
        let mut session = Session::new();
        session.handshake_init(0x500);
        assert_eq!(session.handshake_poll(), Action::Send);
        assert_eq!(session.out().len(), 79);

        let reply = crate::parser::fake_inbound(MAGIC_RSP, 1, frame::OP_HELLO, &[0, 0]);
        session.feed(&reply);
        assert_eq!(session.handshake_poll(), Action::Send);
        assert_eq!(session.out().len(), 34);
    }

    #[test]
    fn display_area_round_trip_decodes_the_three_corners() {
        let frame = frame::set_display_area(1, 290.0, 170.0, -145.0, 10.0, 0.0);
        let payload = &frame[32..];
        let corners = decode_display_area(payload).unwrap();
        assert!((corners[0][0] - -145.0).abs() < 1e-6);
        assert!((corners[0][1] - 180.0).abs() < 1e-6);
        assert!((corners[2][1] - 10.0).abs() < 1e-6);
        assert!((corners[1][0] - 145.0).abs() < 1e-6);
    }

    #[test]
    fn client_responses_are_held_during_the_handshake() {
        let mut session = Session::new();
        session.handshake_init(0x500);
        assert_eq!(session.handshake_poll(), Action::Send);
        let reply = crate::parser::fake_inbound(MAGIC_RSP, 1, frame::OP_HELLO, &[0, 0, 1]);
        let events = session.feed(&reply);
        assert!(events.iter().all(|event| !matches!(event, Feed::Response { .. })));
        assert!(session.take_held().is_some());
    }
}
