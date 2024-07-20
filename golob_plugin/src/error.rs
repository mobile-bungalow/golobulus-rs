use after_effects::OutData;
use log::{error, info};

use crate::GlobalPlugin;

// Utilities for printing and displaying errors.

type RunOutput = Result<Option<String>, golob_lib::GolobulError>;

pub fn handle_run_output(
    instance: &mut crate::instance::Instance,
    global: &GlobalPlugin,
    time: i32,
    out: RunOutput,
) {
    let mut map = match global.errors.get_mut(&instance.id) {
        Some(entry) => entry,
        None => {
            global.errors.insert(instance.id, Default::default());
            global.errors.get_mut(&instance.id).unwrap()
        }
    };

    map.remove(&time);

    match out {
        Ok(Some(out)) => {
            info!("{}", out.trim_end());
            map.insert(
                time,
                crate::instance::DebugContents {
                    error: None,
                    stdout: Some(out),
                },
            );
        }
        Err(e) => {
            error!("Error: {e:?}");
            match e {
                golob_lib::GolobulError::RuntimeError { stderr, stdout } => {
                    if stdout.is_some() {
                        info!("{}", stdout.as_ref().unwrap().trim_end());
                    }

                    map.insert(
                        time,
                        crate::instance::DebugContents {
                            error: Some(stderr),
                            stdout,
                        },
                    );
                }
                e @ golob_lib::GolobulError::OutputSizeTooLarge { .. } => {
                    map.insert(
                        time,
                        crate::instance::DebugContents {
                            error: Some(format!("{e}")),
                            stdout: None,
                        },
                    );
                }
                _ => {}
            };
        }
        _ => {}
    };
}

pub fn startup_error_message(error: golob_lib::GolobulError, out_data: &mut OutData) {
    if let golob_lib::GolobulError::RuntimeError { stderr, stdout } = error {
        error!("{stderr:?}");
        if let Some(stdout) = stdout {
            info!("{}", stdout.trim_end());
            out_data.set_return_msg(&format!(
                "failed to load script: \n error: {stderr} \n stdout: {stdout}"
            ));
        } else {
            out_data.set_return_msg(&format!(" failed to load script: {stderr}"));
        }
    } else {
        error!("{error:?}");
    }
}
