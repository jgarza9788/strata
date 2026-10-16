// SPDX-License-Identifier: MIT

use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use gtk::glib;

/// Parses `zoxide query --list --score`, one `<score> <path>` per line.
pub(crate) fn parse_scores(output: &str) -> Vec<(PathBuf, f64)> {
    output
        .lines()
        .filter_map(|line| {
            let (score, path) = line.trim_start().split_once(' ')?;
            let score = score.parse::<f64>().ok()?;
            let path = PathBuf::from(path);
            (score.is_finite() && path.is_absolute()).then_some((path, score))
        })
        .collect()
}

/// Reads zoxide's folders off the GTK thread when zoxide is installed, then calls
/// `loaded` on the main loop. Nothing is called when zoxide is missing or fails.
pub(crate) fn load(loaded: impl FnOnce(Vec<(PathBuf, f64)>) + 'static) {
    let Some(program) = glib::find_program_in_path("zoxide") else {
        return;
    };
    glib::spawn_future_local(async move {
        let output = gtk::gio::spawn_blocking(move || {
            Command::new(program)
                .args(["query", "--list", "--score"])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
        })
        .await;
        match output {
            Ok(Ok(output)) if output.status.success() => {
                loaded(parse_scores(&String::from_utf8_lossy(&output.stdout)));
            }
            Ok(Ok(output)) => tracing::debug!(status = %output.status, "zoxide query failed"),
            Ok(Err(error)) => tracing::debug!(%error, "zoxide could not run"),
            Err(_) => tracing::debug!("zoxide query panicked"),
        }
    });
}

#[cfg(test)]
mod tests;
