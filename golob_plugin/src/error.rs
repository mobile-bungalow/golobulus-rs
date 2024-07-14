use after_effects::OutData;
use log::{error, info};

// Utilities for printing and displaying errors.

type RunOutput = Result<Option<String>, golob_lib::GolobulError>;

pub fn handle_run_output(instance: &mut crate::instance::Instance, time: i32, out: RunOutput) {
    instance.timestamped_error_log.remove(&time);
    instance.timestamped_log.remove(&time);

    match out {
        Ok(Some(out)) => {
            info!("{}", out.trim_end());
            instance.timestamped_log.insert(time, out);
        }
        Err(e) => {
            if let golob_lib::GolobulError::RuntimeError { stderr, stdout } = e {
                if let Some(stdout) = stdout {
                    info!("{}", stdout.trim_end());
                    instance.timestamped_log.insert(time, stdout);
                }
                error!(" {stderr:?}");
                instance.timestamped_error_log.insert(time, stderr);
            } else {
                error!(" {e:?}");
            }
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
