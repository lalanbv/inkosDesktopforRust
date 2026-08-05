pub const DEFAULT_STUDIO_PORT: u16 = 4567;
pub const HEALTH_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
pub const HEALTH_PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);
pub const CLI_ENTRY_REL: &str = "packages/cli/dist/index.js";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_port_matches_inkos() {
        assert_eq!(DEFAULT_STUDIO_PORT, 4567);
    }
    #[test]
    fn cli_entry_is_studio_dist() {
        assert_eq!(CLI_ENTRY_REL, "packages/cli/dist/index.js");
    }
}
