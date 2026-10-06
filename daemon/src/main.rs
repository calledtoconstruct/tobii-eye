use std::net::SocketAddr;

fn main() {
    let mut init_config = false;
    let mut websocket: Option<SocketAddr> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--init-config" {
            init_config = true;
        } else if arg == "--ws" {
            websocket = Some(parse_ws(args.next().as_deref()));
        } else {
            eprintln!("usage: tobiifreed [--init-config] [--ws [addr:port]]");
            std::process::exit(2);
        }
    }
    if init_config {
        if let Err(err) = tobiifreed::config::write_default_config() {
            eprintln!("tobiifreed: {err}");
            std::process::exit(1);
        }
        return;
    }
    tobiifreed::serve::run(websocket);
}

fn parse_ws(arg: Option<&str>) -> SocketAddr {
    let Some(arg) = arg else {
        return "127.0.0.1:7081".parse().unwrap();
    };
    if let Ok(port) = arg.parse::<u16>() {
        return SocketAddr::from(([127, 0, 0, 1], port));
    }
    arg.parse().unwrap_or_else(|_| "127.0.0.1:7081".parse().unwrap())
}
