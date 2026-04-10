use std::path::Path;

#[cfg(not(target_os = "windows"))]
pub(crate) const ALLOWED_SHELLS: &[&str] = &[
    "/bin/sh",
    "/bin/bash",
    "/bin/zsh",
    "/bin/fish",
    "/bin/dash",
    "/bin/ksh",
    "/bin/tcsh",
    "/bin/csh",
    "/usr/bin/sh",
    "/usr/bin/bash",
    "/usr/bin/zsh",
    "/usr/bin/fish",
    "/usr/bin/dash",
    "/usr/bin/ksh",
    "/usr/bin/tcsh",
    "/usr/bin/csh",
    "/usr/local/bin/bash",
    "/usr/local/bin/zsh",
    "/usr/local/bin/fish",
    "/run/current-system/sw/bin/bash",
    "/run/current-system/sw/bin/zsh",
    "/run/current-system/sw/bin/fish",
];

#[cfg(not(target_os = "windows"))]
pub(crate) const DEFAULT_SHELL: &str = "/bin/zsh";

#[cfg(target_os = "windows")]
const DEFAULT_SHELL_WINDOWS: &str = "powershell.exe";

#[cfg(not(target_os = "windows"))]
pub(crate) fn get_validated_shell() -> String {
    let shell = match std::env::var("SHELL") {
        Ok(s) => s,
        Err(_) => {
            tracing::debug!("SHELL not set, using default: {}", DEFAULT_SHELL);
            return DEFAULT_SHELL.to_string();
        }
    };

    if !shell.starts_with('/') {
        tracing::warn!(
            "SHELL is not an absolute path '{}', using default: {}",
            shell,
            DEFAULT_SHELL
        );
        return DEFAULT_SHELL.to_string();
    }

    if !Path::new(&shell).exists() {
        tracing::warn!(
            "SHELL does not exist '{}', using default: {}",
            shell,
            DEFAULT_SHELL
        );
        return DEFAULT_SHELL.to_string();
    }

    if ALLOWED_SHELLS.contains(&shell.as_str()) {
        return shell;
    }

    if let Ok(resolved) = std::fs::canonicalize(&shell) {
        let resolved_str = resolved.to_string_lossy();
        if ALLOWED_SHELLS
            .iter()
            .any(|&allowed| resolved_str.ends_with(allowed.rsplit('/').next().unwrap_or("")))
        {
            tracing::debug!(
                "SHELL '{}' resolves to allowed shell '{}'",
                shell,
                resolved_str
            );
            return shell;
        }
    }

    tracing::warn!(
        "SHELL '{}' not in allowed list, using default: {}",
        shell,
        DEFAULT_SHELL
    );
    DEFAULT_SHELL.to_string()
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum WindowsShell {
    #[default]
    PowerShell,
    PowerShellCore,
    Cmd,
}

#[cfg(target_os = "windows")]
pub(crate) fn get_validated_shell() -> String {
    let preferred = WindowsShell::default();
    tracing::debug!("User preferred shell: {:?}", preferred);

    if let Ok(system_root) = std::env::var("SystemRoot") {
        match preferred {
            WindowsShell::PowerShellCore => {
                if let Ok(output) = std::process::Command::new("where").arg("pwsh.exe").output() {
                    if output.status.success() {
                        let path = String::from_utf8_lossy(&output.stdout);
                        if let Some(first_line) = path.lines().next() {
                            let pwsh_path = first_line.trim();
                            if Path::new(pwsh_path).exists() {
                                tracing::debug!("Using PowerShell Core: {}", pwsh_path);
                                return pwsh_path.to_string();
                            }
                        }
                    }
                }
                tracing::warn!("PowerShell Core (pwsh) not found, falling back to PowerShell");
                let powershell_path = format!(
                    "{}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
                    system_root
                );
                if Path::new(&powershell_path).exists() {
                    return powershell_path;
                }
            }
            WindowsShell::PowerShell => {
                let powershell_path = format!(
                    "{}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
                    system_root
                );
                if Path::new(&powershell_path).exists() {
                    tracing::debug!("Using Windows PowerShell: {}", powershell_path);
                    return powershell_path;
                }
            }
            WindowsShell::Cmd => {
                let cmd_path = format!("{}\\System32\\cmd.exe", system_root);
                if Path::new(&cmd_path).exists() {
                    tracing::debug!("Using cmd.exe: {}", cmd_path);
                    return cmd_path;
                }
            }
        }

        let powershell_path = format!(
            "{}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
            system_root
        );
        if Path::new(&powershell_path).exists() {
            tracing::debug!("Fallback to Windows PowerShell: {}", powershell_path);
            return powershell_path;
        }

        let cmd_path = format!("{}\\System32\\cmd.exe", system_root);
        if Path::new(&cmd_path).exists() {
            tracing::debug!("Fallback to cmd.exe: {}", cmd_path);
            return cmd_path;
        }
    }

    tracing::warn!(
        "Could not find any shell, using default: {}",
        DEFAULT_SHELL_WINDOWS
    );
    DEFAULT_SHELL_WINDOWS.to_string()
}
