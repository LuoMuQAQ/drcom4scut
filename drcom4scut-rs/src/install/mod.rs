//! 安装/卸载共用逻辑。

pub mod acl;
pub mod driver;
pub mod driver_flow;
pub mod flow;
pub mod identity;
pub mod knownfolder;
pub mod maintenance;
pub mod migrate;
pub mod origin;
pub mod pe;
pub mod process;
pub mod registry;
pub mod selfdelete;
pub mod shortcuts;
pub mod sid;
pub mod transaction;
pub mod ui;
pub mod validate;

pub const APP_DISPLAY_NAME: &str = "校园网认证客户端";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_VERSION_QUAD: &str = "0.3.1.0";
pub const GUI_EXE_NAME: &str = "drcom4scutGUI.exe";
pub const UNINSTALL_EXE_NAME: &str = "uninstall.exe";
pub const SETUP_MUTEX_NAME: &str = "Local\\drcom4scutGUI-Setup";
pub const UNINSTALL_MUTEX_NAME: &str = "Local\\drcom4scutGUI-Uninstall";
