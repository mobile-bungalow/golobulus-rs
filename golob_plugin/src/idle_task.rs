use crate::{
    background_task::{self, BackgroundTask, ImageBuffer, JobId},
    footage_utils,
};
use after_effects::{self as ae, *};
use after_effects_sys as ae_sys;
use dashmap::try_result::TryResult;
use golob_lib::PythonRunner;
use std::{sync::Arc, vec};
use sys::SPBasicSuite;

type IdleHook<T> = fn(&mut T, &mut i32) -> Result<(), Error>;
struct IdleData<T> {
    hook: IdleHook<T>,
    data: T,
}

// This function runs periodically *on the main thread*. At startlingly
// regular intervals, usually around every three frames unless instructed otherwise.
// It's important to keep this under about 16 ms of execution time because it
// blocks the event loop.
fn idle_callback(
    idle_task_info: &mut IdleTaskInfo,
    _max_sleep_time: &mut i32,
) -> Result<(), Error> {

    let keys = idle_task_info
        .task_map
        .iter()
        .map(|x| *x.key())
        .collect::<Vec<_>>();

    let mut dead_keys = vec![];
    let mut cleanup_tasks = vec![];

    for key in keys.iter() {
        let TryResult::Present(mut background_task) = idle_task_info.task_map.try_get_mut(key)
        else {
            log::debug!("bg task {key} locked, continuing.");
            continue;
        };

        match &background_task.status {
            crate::background_task::TaskStatus::Busy => continue,
            crate::background_task::TaskStatus::Ready => {
                log::debug!("bg task {key} ready, sending a job.");
                let res: Result<(), ae::Error> = crate::MAIN_THREAD_IDLE_DATA.with(|data| {
                    let mut data = data.borrow_mut();

                    let Some(main_thread_data) = data.get_mut(key) else {
                        log::error!("Missing main thread data in background job! bailing.");
                        return Err(ae::Error::Generic);
                    };

                    let job = main_thread_data
                        .task_creation_ctx
                        .create_job(&mut background_task.buffers)?;

                    let _ = background_task.tx.send(job);

                    Ok(())
                });

                if res.is_err() {
                    dead_keys.push(key);
                }

                res?;
            }
            crate::background_task::TaskStatus::Done => {
                log::debug!("bg task {key} done, loading footage.");
                let e: Result<(), ae::Error> = crate::MAIN_THREAD_IDLE_DATA.with(|data| {
                    if let Some(cleanup_task) = data.borrow_mut().remove(key) {
                        cleanup_tasks.push(cleanup_task);
                    }
                    Ok(())
                });

                dead_keys.push(key);

                e?;

                continue;
            }
            crate::background_task::TaskStatus::Cancelled => {
                dead_keys.push(key);
                continue;
            }
            crate::background_task::TaskStatus::Error { stdout, error } => {
                log::error!("Error in background task: {}", error);

                dead_keys.push(key);

                let stdout = stdout
                    .as_ref()
                    .map(|s| format!(", Stdout: {s}"))
                    .unwrap_or_default();

                let util = ae::aegp::suites::Utility::new()?;

                util.report_info(
                    *crate::PLUGIN_ID.get().unwrap(),
                    &format!("Render Failed! Error: {error} {stdout}"),
                )?;

                continue;
            }
        }
    }

    for task in cleanup_tasks {
        footage_utils::import_footage(task.on_complete)?;
    }

    for key in dead_keys.iter() {
        idle_task_info.cancel(**key);
    }

    if !keys.is_empty() {
        let adv_app = ae::pf::suites::AdvApp::new()?;
        adv_app.refresh_all_windows()?;
    }

    // This forces the ui to update *immediately*
    // so we have to make sure we aren't holding any locks.
    Ok(())
}

// Data which should be kept on the main thread
// includes handles which may lose validity.
pub struct IdleTaskBundle {
    pub on_complete: footage_utils::FootageImportTask,
    pub task_creation_ctx: TaskCreationCtx,
}

pub struct TaskCreationCtx {
    pub effect: ae::aegp::EffectRefHandle,
    pub comp: ae::aegp::CompHandle,
    pub param_indices: Vec<(i32, String, golob_lib::Variant)>,
    pub current_frame: u32,
    pub total_frames: u32,
    pub current_time: Time,
    pub time_step: i32,
}

impl TaskCreationCtx {
    pub fn new(
        runner: &PythonRunner,
        in_data: &mut InData,
        effect: ae::aegp::EffectRefHandle,
        comp: ae::aegp::CompHandle,
        total_frames: u32,
    ) -> Self {
        let param_indices = runner
            .iter_inputs()
            .enumerate()
            .map(|(i, (name, ty))| {
                (
                    crate::param_util::as_param_index(i, ty).idx(),
                    name.clone(),
                    ty.clone(),
                )
            })
            .collect();

        let value = in_data.current_time();
        let scale = in_data.time_scale();
        TaskCreationCtx {
            effect,
            comp,
            param_indices,
            current_time: ae::Time { value, scale },
            time_step: in_data.time_step(),
            current_frame: 0,
            total_frames,
        }
    }

    pub fn create_job(
        &mut self,
        shared_buffers: &mut Vec<ImageBuffer>,
    ) -> Result<background_task::TaskMessage, ae::Error> {
        let mut inputs = vec![];

        let stream_suite = ae::aegp::suites::Stream::new()?;
        let layer_suite = ae::aegp::suites::Layer::new()?;
        let plugin_id = *crate::PLUGIN_ID.get().unwrap();

        for (param_index, name, variant) in self.param_indices.iter() {
            let stream =
                stream_suite.new_effect_stream_by_index(self.effect, plugin_id, *param_index)?;

            let stream_val = stream_suite.new_stream_value(
                stream,
                plugin_id,
                aegp::TimeMode::LayerTime,
                self.current_time,
                false,
            )?;

            if let (golob_lib::Variant::Image(_), ae::aegp::StreamValue::LayerId(id)) =
                (variant, stream_val)
            {
                let layer_handle = layer_suite.layer_from_layer_id(&self.comp, id as u32)?;
                let shared_buffer =
                    if let Some(i) = shared_buffers.iter().position(|buf| buf.name == *name) {
                        &mut shared_buffers[i]
                    } else {
                        shared_buffers.push(ImageBuffer::blank(name.to_owned()));
                        shared_buffers.last_mut().unwrap()
                    };

                footage_utils::get_layer_pixels(shared_buffer, &layer_handle, self.current_time)?;
            } else {
                let mut variant = variant.clone();
                let _ = crate::param_util::set_variant_from_stream_val(&mut variant, stream_val);
                inputs.push((name.clone(), variant));
            }
        }

        let job = background_task::TaskMessage::Job {
            inputs,
            time: self.current_time.value as f32 / self.current_time.scale as f32,
            frame: self.current_frame,
        };

        self.current_frame += 1;
        self.current_time.value += self.time_step;

        Ok(job)
    }
}

pub struct IdleTaskInfo {
    pub task_map: Arc<dashmap::DashMap<JobId, BackgroundTask>>,
}

impl IdleTaskInfo {
    pub fn cancel(&self, id: JobId) {
        log::debug!("Cancelling task {id}.");
        // Remove the directory of in progress pngs if it exists
        crate::MAIN_THREAD_IDLE_DATA.with(|data| {
            data.borrow_mut().remove(&id);
        });

        // Remove the task from the task map
        let _nope = self.task_map.remove(&id);

        if _nope.is_none() {
            log::warn!("Task not present while bailing.");
        }
    }
}

pub fn register(bundle: IdleTaskInfo) -> Result<(), Error> {
    let suite = RegisterSuite::new(crate::IDLE_TASK_PICA.get().unwrap())?;

    suite.register_idle_hook(*crate::PLUGIN_ID.get().unwrap(), idle_callback, bundle)?;
    Ok(())
}

unsafe extern "C" fn unpack_function<T>(
    _global_void_pointer: ae_sys::AEGP_GlobalRefcon, // null
    void_pointer: ae_sys::AEGP_IdleRefcon,
    max_sleep_ptr: *mut ae_sys::A_long,
) -> i32 {
    let Some(in_data) = crate::IDLE_TASK_PICA.get() else {
        log::error!("No idle task pica found, This should have been set in global setup.");
        unreachable!();
    };
    // This handle RAII replaces the old ptr at the end of the function when it gets dropped.
    let _replace_handle = after_effects::PicaBasicSuite::from_sp_basic_suite_raw(in_data);
    let IdleData { hook, ref mut data } = *(void_pointer as *mut IdleData<T>);
    let sleep_time = &mut (*max_sleep_ptr);

    match hook(data, sleep_time) {
        Ok(()) => 0,
        Err(e) => e.into(),
    }
}

struct RegisterSuite {
    pica_basic_suite_ptr: *const ae_sys::SPBasicSuite,
    suite_ptr: *const ae_sys::AEGP_RegisterSuite5,
}

impl RegisterSuite {
    fn new(pica_basic_suite_ptr: *const SPBasicSuite) -> Result<Self, Error> {
        unsafe {
            let mut suite_ptr =
                std::mem::MaybeUninit::<*const ae_sys::AEGP_RegisterSuite5>::uninit();

            let aquire_suite = (*(pica_basic_suite_ptr)).AcquireSuite.unwrap();

            aquire_suite(
                ae_sys::kAEGPRegisterSuite.as_ptr() as *const i8,
                ae_sys::kAEGPRegisterSuiteVersion5 as i32,
                suite_ptr.as_mut_ptr() as _,
            );

            Ok(Self {
                pica_basic_suite_ptr,
                suite_ptr: suite_ptr.assume_init(),
            })
        }
    }

    fn register_idle_hook<T>(
        &self,
        plugin_id: i32,
        cb: IdleHook<T>,
        idle_state: T,
    ) -> Result<(), Error> {
        let mut idle_stuff = Box::new(IdleData {
            hook: cb,
            data: idle_state,
        });

        let res = Ok({
            let err = unsafe {
                let idle_hook = (*self.suite_ptr).AEGP_RegisterIdleHook.unwrap();

                idle_hook(
                    plugin_id,
                    Some(unpack_function::<T>),
                    idle_stuff.as_mut() as *mut _ as *mut _,
                )
            };
            match err {
                0 => Ok(()),
                _ => Err(Error::from(err)),
            }
        }?);

        // this is really the epitome of bad rust.
        std::mem::forget(idle_stuff);
        res
    }
}

impl Drop for RegisterSuite {
    fn drop(&mut self) {
        unsafe {
            let release_suite_func = (*(self.pica_basic_suite_ptr))
                .ReleaseSuite
                .unwrap_or_else(|| unreachable!());
            release_suite_func(
                after_effects_sys::kAEGPRegisterSuite.as_ptr() as *const i8,
                after_effects_sys::kAEGPRegisterSuiteVersion5 as i32,
            );
        };
    }
}
