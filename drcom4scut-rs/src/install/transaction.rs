//! 安装文件事务：暂存、提交、失败回滚。不删除用户已有数据或共享驱动。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Journal {
    pub created_files: Vec<String>,
    pub created_dirs: Vec<String>,
    pub backups: Vec<BackupEntry>,
    pub committed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub original: String,
    pub backup: String,
}

pub struct Transaction {
    pub dest: PathBuf,
    pub staging: PathBuf,
    pub journal: Journal,
    pub is_upgrade: bool,
}

impl Transaction {
    pub fn begin(dest: &Path) -> Result<Self, String> {
        let staging = dest.join(".install-staging");
        if staging.exists() {
            let _ = std::fs::remove_dir_all(&staging);
        }
        std::fs::create_dir_all(&staging).map_err(|e| format!("无法创建暂存目录：{e}"))?;
        Ok(Self {
            dest: dest.to_path_buf(),
            staging,
            journal: Journal::default(),
            is_upgrade: dest.join("install-state.json").is_file(),
        })
    }

    pub fn stage_file(&mut self, rel: &str, bytes: &[u8]) -> Result<(), String> {
        let staged = self.staging.join(rel);
        if let Some(parent) = staged.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&staged, bytes).map_err(|e| format!("写入暂存失败：{e}"))?;
        Ok(())
    }

    pub fn commit(&mut self) -> Result<(), String> {
        let files = collect_files(&self.staging)?;
        for rel in &files {
            let from = self.staging.join(rel);
            let to = self.dest.join(rel);
            if let Some(parent) = to.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    self.journal
                        .created_dirs
                        .push(parent.to_string_lossy().into_owned());
                }
            }
            if to.exists() {
                let bak = to.with_extension("bak-upgrade");
                std::fs::copy(&to, &bak).map_err(|e| format!("备份失败：{e}"))?;
                self.journal.backups.push(BackupEntry {
                    original: to.to_string_lossy().into_owned(),
                    backup: bak.to_string_lossy().into_owned(),
                });
            } else {
                self.journal
                    .created_files
                    .push(to.to_string_lossy().into_owned());
            }
            std::fs::copy(&from, &to).map_err(|e| format!("提交 {rel} 失败：{e}"))?;
        }
        self.journal.committed = true;
        for b in &self.journal.backups {
            let _ = std::fs::remove_file(&b.backup);
        }
        let _ = std::fs::remove_dir_all(&self.staging);
        Ok(())
    }

    /// 失败回滚：新文件删除，升级备份恢复。不触碰 data\ 与共享驱动。
    pub fn rollback(&self) -> Result<(), String> {
        for f in self.journal.created_files.iter().rev() {
            let p = Path::new(f);
            if is_protected_user_path(p, &self.dest) {
                continue;
            }
            let _ = std::fs::remove_file(p);
        }
        for b in &self.journal.backups {
            let _ = std::fs::copy(&b.backup, &b.original);
            let _ = std::fs::remove_file(&b.backup);
        }
        let _ = std::fs::remove_dir_all(&self.staging);
        Ok(())
    }
}

fn is_protected_user_path(path: &Path, dest: &Path) -> bool {
    crate::install::validate::is_under(path, &dest.join("data"))
}

fn collect_files(root: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, out)?;
            } else {
                let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
                out.push(rel.to_string_lossy().replace('/', "\\"));
            }
        }
        Ok(())
    }
    walk(root, root, &mut out)?;
    Ok(out)
}

pub fn remove_empty_owned_dirs(dest: &Path, dirs: &[String]) {
    for d in dirs.iter().rev() {
        let p = dest.join(d);
        if p.is_dir() {
            let _ = std::fs::remove_dir(p); // 非空则失败并保留
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "drcom-tx-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn commit_then_rollback_new_install() {
        let dest = tmp("new");
        let mut tx = Transaction::begin(&dest).unwrap();
        tx.stage_file("drcom4scutGUI.exe", b"gui").unwrap();
        tx.stage_file(r"runtime\aa\drcom4scut.exe", b"core")
            .unwrap();
        tx.commit().unwrap();
        assert!(dest.join("drcom4scutGUI.exe").is_file());
        tx.rollback().unwrap();
        assert!(!dest.join("drcom4scutGUI.exe").exists());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn upgrade_commit_drops_backup_and_keeps_data() {
        let dest = tmp("upg");
        std::fs::write(dest.join("drcom4scutGUI.exe"), b"old").unwrap();
        std::fs::create_dir_all(dest.join("data").join("users").join("S-1")).unwrap();
        std::fs::write(
            dest.join("data")
                .join("users")
                .join("S-1")
                .join("settings.v1.dat"),
            b"secret",
        )
        .unwrap();
        let mut tx = Transaction::begin(&dest).unwrap();
        tx.stage_file("drcom4scutGUI.exe", b"new").unwrap();
        tx.commit().unwrap();
        assert_eq!(
            std::fs::read(dest.join("drcom4scutGUI.exe")).unwrap(),
            b"new"
        );
        assert!(
            !dest.join("drcom4scutGUI.bak-upgrade").exists(),
            "提交成功后必须删掉升级备份"
        );
        assert_eq!(
            std::fs::read(
                dest.join("data")
                    .join("users")
                    .join("S-1")
                    .join("settings.v1.dat")
            )
            .unwrap(),
            b"secret"
        );
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn rollback_does_not_delete_data_even_if_journal_lists_it() {
        let dest = tmp("data");
        let data = dest
            .join("data")
            .join("users")
            .join("S-1")
            .join("settings.v1.dat");
        std::fs::create_dir_all(data.parent().unwrap()).unwrap();
        std::fs::write(&data, b"keep").unwrap();
        let tx = Transaction {
            dest: dest.clone(),
            staging: dest.join(".install-staging"),
            journal: Journal {
                created_files: vec![data.to_string_lossy().into_owned()],
                created_dirs: vec![],
                backups: vec![],
                committed: false,
            },
            is_upgrade: true,
        };
        tx.rollback().unwrap();
        assert!(data.is_file());
        let _ = std::fs::remove_dir_all(&dest);
    }
}
