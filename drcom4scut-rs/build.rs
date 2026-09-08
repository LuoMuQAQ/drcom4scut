//! 构建脚本：嵌入 Windows manifest 和图标资源。

fn main() {
    println!("cargo:rustc-check-cfg=cfg(embed_payload)");
    println!("cargo:rerun-if-env-changed=DRCOM_GUI_EXE");
    println!("cargo:rerun-if-env-changed=DRCOM_UNINSTALL_EXE");
    if let (Ok(gui), Ok(un)) = (
        std::env::var("DRCOM_GUI_EXE"),
        std::env::var("DRCOM_UNINSTALL_EXE"),
    ) {
        if !gui.is_empty() && !un.is_empty() {
            println!("cargo:rerun-if-changed={gui}");
            println!("cargo:rerun-if-changed={un}");
            println!("cargo:rustc-cfg=embed_payload");
            println!("cargo:rustc-env=DRCOM_GUI_EXE={gui}");
            println!("cargo:rustc-env=DRCOM_UNINSTALL_EXE={un}");
        }
    }
    // 仅在 Windows 平台下编译资源
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        // 嵌入 manifest：comctl32 v6 + PerMonitorV2 DPI 感知
        // Only the client starts elevated. Setup must capture the original user
        // before elevating its worker; uninstall already has an explicit runas flow.
        for (binary, level) in [
            ("drcom4scutGUI", "requireAdministrator"),
            ("drcom4scut-Setup", "asInvoker"),
            ("uninstall", "asInvoker"),
        ] {
            embed_manifest(binary, level);
        }

        // 通知 cargo 在 resources 目录变化时重新构建
        println!("cargo:rerun-if-changed=resources/");
    }
}

fn embed_manifest(binary: &str, level: &str) {
    // 生成 manifest 内容
    let manifest = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity
    type="win32"
    name="@BINARY@"
    version="@VERSION@"
    processorArchitecture="amd64"/>

  <description>校园网认证客户端</description>

  <!-- 请求管理员权限（用于开机启动计划任务） -->
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="@LEVEL@" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>

  <!-- 启用 ComCtl32 v6 (视觉样式) -->
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="amd64"
        publicKeyToken="6595b64144ccf1df"
        language="*"/>
    </dependentAssembly>
  </dependency>

  <!-- DPI 感知：PerMonitorV2 -->
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>

  <!-- Windows 10+ 兼容性 -->
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/> <!-- Windows 10 -->
      <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/> <!-- Windows 7 -->
    </application>
  </compatibility>
</assembly>
"#;
    let manifest = manifest
        .replace("@BINARY@", binary)
        .replace("@LEVEL@", level)
        .replace(
            "@VERSION@",
            &format!("{}.0", std::env::var("CARGO_PKG_VERSION").unwrap()),
        );

    // 写入临时 manifest 文件
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let manifest_path = std::path::Path::new(&out_dir).join(format!("{binary}.manifest"));
    std::fs::write(&manifest_path, manifest).expect("无法写入 manifest");

    // 使用 embed-resource crate 嵌入 manifest
    #[cfg(target_env = "gnu")]
    {
        // GNU 工具链使用 windres
        embed_with_windres(binary, &manifest_path);
    }
}

#[cfg(target_env = "gnu")]
fn embed_with_windres(binary: &str, manifest_path: &std::path::Path) {
    use std::io::Write;

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let rc_path = std::path::Path::new(&out_dir).join(format!("{binary}.rc"));

    // 生成 .rc 文件
    let rc_content = format!(
        r#"
// Manifest
1 24 "{}"

// 图标
1 ICON "resources/app.ico"

// License for the embedded eye / eye-off SVG artwork.
102 RCDATA "resources/lucide-LICENSE"
"#,
        manifest_path.display().to_string().replace("\\", "\\\\")
    );

    let mut rc_file = std::fs::File::create(&rc_path).expect("无法创建 .rc 文件");
    rc_file
        .write_all(rc_content.as_bytes())
        .expect("无法写入 .rc 文件");

    // 调用 windres 编译资源
    let obj_path = std::path::Path::new(&out_dir).join(format!("{binary}.o"));
    let status = std::process::Command::new("windres")
        .arg(&rc_path)
        .arg(&obj_path)
        .status();

    match status {
        Ok(s) if s.success() => {
            // 链接生成的 .o 文件
            println!("cargo:rustc-link-arg-bin={binary}={}", obj_path.display());
            if binary == "uninstall" {
                // Unit tests need the same visual styles/DPI resources, but must
                // remain asInvoker. Only the cfg(test) library links this archive.
                let archive = std::path::Path::new(&out_dir).join("libtest_resources.a");
                let status = std::process::Command::new("ar")
                    .arg("crs")
                    .arg(&archive)
                    .arg(&obj_path)
                    .status()
                    .expect("无法启动 ar");
                assert!(status.success(), "无法构建单元测试资源");
                println!("cargo:rustc-link-search=native={out_dir}");
            }
        }
        _ => {
            panic!("windres 调用失败：不能发布缺少权限清单和图标的 {binary}");
        }
    }
}
