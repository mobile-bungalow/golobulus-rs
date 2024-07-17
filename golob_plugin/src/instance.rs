use crate::background_task::BackgroundTask;
use crate::background_task::JobId;
use crate::background_task::OutputDesc;
use crate::footage_utils;
use crate::footage_utils::create_suffixed_directory;
use crate::idle_task;
use crate::param_util;
use crate::ParamIdx;
use crate::PluginState;
use crate::INPUT_LAYER_CHECKOUT_ID;
use after_effects as ae;
use after_effects::*;
use after_effects_sys as ae_sys;
use golob_lib::{ImageFormat, InDesc, OutDesc, PythonRunner};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

after_effects::define_cross_thread_type!(Instance);

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Instance {
    #[serde(skip_serializing, skip_deserializing)]
    pub runner: PythonRunner,
    pub src: Option<String>,
    pub last_known_path: Option<PathBuf>,
    pub venv_path: Option<PathBuf>,
    #[serde(skip_serializing, skip_deserializing)]
    pub job_id: Option<JobId>,
    #[serde(skip_serializing, skip_deserializing)]
    pub timestamped_error_log: HashMap<i32, String>,
    #[serde(skip_serializing, skip_deserializing)]
    pub timestamped_log: HashMap<i32, String>,
}

impl Instance {
    pub fn launch_script_dialog(&mut self, out_data: &mut OutData) -> Result<(), Error> {
        let projec_dir = footage_utils::get_project_dir();

        let is_saved = projec_dir.is_some();

        let home_dir = projec_dir.unwrap_or_else(|| match homedir::get_my_home() {
            Ok(Some(home)) => home,
            _ => "/".into(),
        });

        let file = rfd::FileDialog::new()
            .add_filter("python script", &["py"])
            .set_directory(home_dir.clone())
            .pick_file();

        let Some(file) = file else {
            return Ok(());
        };

        let source = match std::fs::read_to_string(file.clone()) {
            Err(e) => {
                out_data.set_error_msg(&format!("Error reading script file: {e}"));
                return Err(Error::Generic);
            }
            Ok(src) => src,
        };

        let file_path = file.clone().to_str().unwrap().to_owned();

        if let Some(parent) = file.parent().and_then(|p| p.canonicalize().ok()) {
            self.runner.set_script_parent_directory(parent);
        }

        self.runner
            .load_script(&source, Some(file_path.clone()))
            .map_err(|e| {
                crate::error::startup_error_message(e, out_data);
                Error::None
            })?;

        self.timestamped_error_log.clear();
        self.timestamped_log.clear();
        self.src = Some(source);

        if is_saved {
            let new_path = pathdiff::diff_paths(file.clone(), home_dir);
            if let Some(new_path) = new_path {
                self.last_known_path = Some(new_path);
            } else {
                self.last_known_path = Some(file);
            }
        } else {
            self.last_known_path = Some(file);
        }

        Ok(())
    }

    pub fn launch_venv_dialog(&mut self) -> Result<(), Error> {
        let home_dir = match homedir::get_my_home() {
            Ok(Some(home)) => home,
            _ => "/".into(),
        };

        let Some(file_path) = rfd::FileDialog::new().set_directory(home_dir).pick_folder() else {
            return Ok(());
        };

        self.venv_path = Some(file_path.clone());
        self.runner.set_venv_path(file_path);

        Ok(())
    }

    pub fn try_reload(&mut self, out_data: &mut OutData) -> Result<(), Error> {
        if let Some(ref file_path) = self.last_known_path {
            let source = std::fs::read_to_string(file_path).ok().unwrap_or_default();

            if source.is_empty() {
                return Err(Error::Generic);
            }

            let file_path = file_path.file_name().unwrap().to_str().unwrap();
            self.runner
                .load_script(&source, Some(file_path.to_owned()))
                .map_err(|e| {
                    crate::error::startup_error_message(e, out_data);
                    Error::Generic
                })?;

            self.timestamped_error_log.clear();
            self.timestamped_log.clear();
            self.src = Some(source);
            Ok(())
        } else {
            Err(Error::Generic)
        }
    }

    pub fn smart_render(
        &mut self,
        in_data: &InData,
        cb: &SmartRenderCallbacks,
    ) -> Result<(), Error> {
        if self.runner.is_sequential() {
            let in_layer = cb.checkout_layer_pixels(INPUT_LAYER_CHECKOUT_ID.idx() as u32)?;
            let mut out_layer = cb.checkout_output()?;
            out_layer.copy_from(&in_layer, None, None)?;
            return Ok(());
        }

        let layers = crate::param_util::set_params(in_data, &mut self.runner)?;

        let layers: Vec<_> = layers
            .iter()
            .filter_map(|(name, index)| {
                Some((name, cb.checkout_layer_pixels(index.idx() as u32).ok()?))
            })
            .collect();

        let mut out_layer = cb.checkout_output()?;

        // Zero the target,
        // we need to do this because
        // requests to scale down could
        // produce garbage memory in the image
        // margins.
        out_layer.fill(None, None)?;

        let stride = out_layer.buffer_stride();

        let output = OutDesc {
            fmt: format(out_layer.bit_depth()),
            width: out_layer.width() as u32,
            height: out_layer.height() as u32,
            data: out_layer.buffer_mut(),
            stride: Some(stride as u32),
        };

        let e = {
            let mut pass = self.runner.create_render_pass(output);

            for (name, layer) in layers.iter() {
                let i = InDesc {
                    fmt: format(layer.bit_depth()),
                    width: layer.width() as u32,
                    height: layer.height() as u32,
                    data: layer.buffer(),
                    stride: Some(layer.buffer_stride() as u32),
                };
                pass.load_input(i, name);
            }

            pass.submit()
        };

        // If we fail, we just become a transparent layer and log the error
        if e.is_err() {
            out_layer.fill(None, None)?;
        }

        crate::error::handle_run_output(self, in_data.current_time(), e);

        Ok(())
    }

    pub fn smart_pre_render(
        &mut self,
        in_data: &InData,
        extra: &mut PreRenderExtra,
    ) -> Result<(), Error> {
        let mut req = extra.output_request();
        let cb = extra.callbacks();

        let current_time = in_data.current_time();
        let time_step = in_data.time_step();
        let time_scale = in_data.time_scale();

        for (index, (_, v)) in self
            .runner
            .iter_inputs()
            .enumerate()
            .filter(|(_, (_, v))| matches!(v, golob_lib::Variant::Image(_)))
        {
            let id_and_index = crate::param_util::as_param_index(index, v).idx();

            cb.checkout_layer(
                id_and_index,
                id_and_index,
                &req,
                current_time,
                time_step,
                time_scale,
            )?;
        }

        req.field = ae_sys::PF_Field_FRAME as i32;
        req.preserve_rgb_of_zero_alpha = 1;
        req.channel_mask = ae_sys::PF_ChannelMask_ARGB as i32;

        // We checkout once just to see what the max rect is :(
        if let Ok(width_test) = cb.checkout_layer(
            0,
            INPUT_LAYER_CHECKOUT_ID.idx() - 1,
            &req,
            in_data.current_time(),
            in_data.time_step(),
            in_data.time_scale(),
        ) {
            req.rect = width_test.max_result_rect;

            let full_checkout = cb.checkout_layer(
                0,
                INPUT_LAYER_CHECKOUT_ID.idx(),
                &req,
                in_data.current_time(),
                in_data.time_step(),
                in_data.time_scale(),
            )?;

            extra.set_result_rect(full_checkout.result_rect.into());
            extra.set_max_result_rect(full_checkout.result_rect.into());
            extra.set_returns_extra_pixels(true);
        }
        Ok(())
    }

    pub fn handle_param_interaction(
        &mut self,
        plugin: &mut PluginState,
        param: ParamIdx,
    ) -> Result<(), Error> {
        match param {
            ParamIdx::LoadButton => {
                self.launch_script_dialog(&mut plugin.out_data)?;
                param_util::update_param_defaults_and_labels(plugin, self)?;
                param_util::update_input_visibilities(plugin, self)?;

                plugin.out_data.set_force_rerender();
            }
            ParamIdx::UnloadButton => {
                // this modify the global shared state between
                // all interpreters so we just unload it.
                // this should be fine unless the user
                // has like, dozens of instances pointing at different
                // directories.
                self.runner
                    .clear_script_parent_directory()
                    .map_err(|_| Error::Generic)?;

                self.runner = PythonRunner::default();
                self.src = None;
                param_util::update_input_visibilities(plugin, self)?;
                plugin.out_data.set_force_rerender();
            }
            ParamIdx::SetVenv => {
                self.launch_venv_dialog()?;

                if self.src.is_some() {
                    self.try_reload(&mut plugin.out_data)?;
                }

                param_util::update_param_defaults_and_labels(plugin, self)?;
                param_util::update_input_visibilities(plugin, self)?;
                plugin.out_data.set_force_rerender();
            }
            ParamIdx::UnsetVenv => {
                self.runner.clear_venv_path().map_err(|_| Error::Generic)?;
                self.venv_path = None;

                if self.src.is_some() {
                    self.try_reload(&mut plugin.out_data)?;
                }

                param_util::update_input_visibilities(plugin, self)?;
                plugin.out_data.set_force_rerender();
            }
            ParamIdx::ReloadButton => {
                self.try_reload(&mut plugin.out_data)?;
                param_util::update_input_visibilities(plugin, self)?;
                plugin.out_data.set_force_rerender();
            }
            ParamIdx::CancelRender => {
                if let Some(job_id) = self.job_id {
                    if let Some(task) = plugin.global.task_map.get(&job_id) {
                        task.cancel();
                    }
                }
            }
            ParamIdx::StartRender => {
                let Some(project_path) = footage_utils::get_project_dir() else {
                    plugin.out_data.set_return_msg("You must save before triggering a golobulus render. This operation will create footage next to your project file.");
                    return Ok(());
                };

                if self
                    .job_id
                    .is_some_and(|id| plugin.global.bg_render_is_active(id))
                {
                    plugin
                        .out_data
                        .set_return_msg("A render is already in progress.");
                    return Ok(());
                }

                let mut directory =
                    footage_utils::output_dir_name(&plugin.in_data.effect_ref(), project_path)?;
                directory =
                    footage_utils::output_dir_name(&plugin.in_data.effect_ref(), directory)?;

                let fmt = footage_utils::get_sequence_output_format()?;
                let frame_count =
                    footage_utils::get_region_of_interest_frame_count(&plugin.in_data)?;

                plugin.global.current_id += 1;
                self.job_id = Some(plugin.global.current_id);

                let pf_interface = ae::aegp::suites::PFInterface::new()?;
                let layer_suite = ae::aegp::suites::Layer::new()?;

                let current_effect = pf_interface.new_effect_for_effect(
                    plugin.in_data.effect_ref(),
                    *crate::PLUGIN_ID.get().unwrap(),
                )?;

                let this_layer = pf_interface.effect_layer(plugin.in_data.effect_ref())?;
                let parent_comp = layer_suite.layer_parent_comp(this_layer)?;

                directory = create_suffixed_directory(&directory);
                // Handling paraminteractions will only ever happen on the main thread.
                let e: Result<(), after_effects::Error> =
                    crate::MAIN_THREAD_IDLE_DATA.with(|data| {
                        let idle_task_info = idle_task::IdleTaskBundle {
                            on_complete: footage_utils::FootageImportTask::new(
                                &plugin.in_data,
                                directory.clone(),
                            )?,
                            task_creation_ctx: idle_task::TaskCreationCtx::new(
                                &self.runner,
                                &mut plugin.in_data,
                                current_effect,
                                parent_comp,
                                frame_count,
                            ),
                        };

                        data.borrow_mut()
                            .insert(plugin.global.current_id, idle_task_info);

                        Ok(())
                    });

                e?;

                BackgroundTask::spawn_task(
                    plugin.global.current_id,
                    self.runner.clone(),
                    OutputDesc {
                        directory,
                        fmt,
                        last_frame: frame_count,
                        width: plugin.in_data.width() as u32,
                        height: plugin.in_data.height() as u32,
                    },
                    plugin.global.task_map.clone(),
                );
            }
            ParamIdx::IsImageFilter => {
                let is_image_filter = plugin
                    .params
                    .get(ParamIdx::IsImageFilter)?
                    .as_checkbox()?
                    .value();

                let first_image = self
                    .runner
                    .iter_inputs()
                    .enumerate()
                    .find(|(_, (_, i))| matches!(i, golob_lib::Variant::Image(_)));

                if let Some((i, (_, ty))) = first_image {
                    let index = param_util::as_param_index(i, ty);

                    if is_image_filter {
                        let mut param = plugin.params.get_mut(index)?;
                        let mut layer = param.as_layer_mut()?;
                        layer.set_default_to_this_layer();
                    };

                    param_util::set_param_visibility(plugin.in_data, index, !is_image_filter)?;

                    param_util::update_input_visibilities(plugin, self)?;
                    plugin.out_data.set_force_rerender();
                }
            }
            ParamIdx::ShowDebug | ParamIdx::DebugOffset | ParamIdx::TemporalWindow => {}
            _ => {}
        };
        Ok(())
    }
}

pub fn format(bit_depth: i16) -> ImageFormat {
    match bit_depth {
        8 => ImageFormat::Argb8,
        16 => ImageFormat::Argb16ae,
        32 => ImageFormat::Argb32,
        _ => unreachable!(),
    }
}
