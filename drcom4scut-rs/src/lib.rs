//! drcom4scut GUI 核心逻辑库。
//!
//! 除 `ui` 模块外，全部为与界面框架无关的 Windows 平台逻辑，便于独立单元测试。

pub mod controller;
pub mod health;
pub mod logtail;
pub mod model;
pub mod paths;

pub mod adapters;
pub mod coreproc;
pub mod install;
pub mod logparse;
pub mod platform;
pub mod settings;

pub mod ui;

// Preserve native-control test activation/DPI while the actual GUI requires admin.
#[cfg(all(test, target_os = "windows", target_env = "gnu"))]
#[link(name = "test_resources", kind = "static", modifiers = "+whole-archive")]
extern "C" {}
