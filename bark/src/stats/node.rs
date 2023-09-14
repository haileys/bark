use bark_protocol::types::stats::node::NodeStats;

pub fn get() -> NodeStats {
    let username = get_username();
    let hostname = get_hostname();

    NodeStats {
        username: as_fixed(&username),
        hostname: as_fixed(&hostname),
    }
}

pub fn display(stats: &NodeStats) -> String {
    let username = from_fixed(&stats.username);
    let hostname = from_fixed(&stats.hostname);
    format!("{username}@{hostname}")
}

fn from_fixed(bytes: &[u8]) -> &str {
    let len = bytes.iter()
        .position(|b| *b == 0)
        .unwrap_or(bytes.len());

    std::str::from_utf8(&bytes[0..len]).unwrap_or_default()
}

fn as_fixed(s: &str) -> [u8; 32] {
    let mut buff = [0u8; 32];
    buff[0..s.as_bytes().len()].copy_from_slice(s.as_bytes());
    buff
}

fn get_username() -> String {
    let uid = nix::unistd::getuid();
    let user = nix::unistd::User::from_uid(uid).ok().flatten();

    user.map(|u| u.name)
        .unwrap_or_else(|| uid.to_string())
}

fn get_hostname() -> String {
    let hostname = nix::unistd::gethostname().ok().unwrap_or_default();
    hostname.to_string_lossy().to_string()
}
