use anyhow::Result;

use crate::dashboard;

pub fn handle_dashboard(addr: Option<String>, no_browser: bool) -> Result<()> {
    dashboard::run_dashboard(addr, !no_browser)
}
