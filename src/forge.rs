pub const HOST: &str = "github.com";

pub fn origin(host: &str, owner: &str) -> String {
    format!("https://{host}/{owner}")
}
