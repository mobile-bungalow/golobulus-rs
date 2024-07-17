use crate::ParamIdx;
use crate::INPUT_LAYER_CHECKOUT_ID;
use ae::aegp::suites;
use ae::aegp::DynamicStreamFlags;
use ae::aegp::StreamValue;
use ae::ParamFlag;
use ae::{Error, InData};
use after_effects as ae;
use after_effects_sys::PF_Pixel;
use golob_lib::Variant;

pub const MAX_INPUTS: i32 = 32;
pub const PARAM_TYPE_COUNT: i32 = 7;
pub const STATIC_PARAMS_OFFSET: i32 = ParamIdx::IsImageFilter.idx() + 1;

impl ParamIdx {
    pub const fn idx(&self) -> i32 {
        match self {
            Self::ScriptGroupStart => 1,
            Self::LoadButton => 2,
            Self::UnloadButton => 3,
            Self::SetVenv => 4,
            Self::UnsetVenv => 5,
            Self::ReloadButton => 6,
            Self::ScriptGroupEnd => 7,
            Self::DebugGroupBegin => 8,
            Self::ShowDebug => 9,
            Self::DebugOffset => 10,
            Self::TemporalWindow => 11,
            Self::DebugGroupEnd => 12,
            Self::ContinuousRenderGroupBegin => 13,
            Self::StartRender => 14,
            Self::CancelRender => 15,
            Self::ContinuousRenderGroupEnd => 16,
            Self::ParametersStart => 17,
            Self::IsImageFilter => 18,
            Self::Dynamic(x) => *x,
            Self::ParametersEnd => Self::IsImageFilter.idx() + (MAX_INPUTS * PARAM_TYPE_COUNT) + 1,
        }
    }
}

impl From<usize> for ParamIdx {
    fn from(value: usize) -> Self {
        match value {
            1 => Self::ScriptGroupStart,
            2 => Self::LoadButton,
            3 => Self::UnloadButton,
            4 => Self::SetVenv,
            5 => Self::UnsetVenv,
            6 => Self::ReloadButton,
            7 => Self::ScriptGroupEnd,
            8 => Self::DebugGroupBegin,
            9 => Self::ShowDebug,
            10 => Self::DebugOffset,
            11 => Self::TemporalWindow,
            12 => Self::DebugGroupEnd,
            13 => Self::ContinuousRenderGroupBegin,
            14 => Self::StartRender,
            15 => Self::CancelRender,
            16 => Self::ContinuousRenderGroupEnd,
            17 => Self::ParametersStart,
            18 => Self::IsImageFilter,
            end if end as i32
                == (Self::IsImageFilter.idx() + (MAX_INPUTS * PARAM_TYPE_COUNT) + 1) =>
            {
                Self::ParametersEnd
            }
            n => Self::Dynamic(n as i32),
        }
    }
}

pub enum AeVariant {
    Float = 0,
    Int,
    IntList,
    Point,
    Bool,
    Color,
    Image,
}
impl AeVariant {}

pub fn as_param_index(index: usize, variant: &Variant) -> ParamIdx {
    let variant: i32 = match variant {
        Variant::Float(_) => AeVariant::Float as _,
        Variant::Int(_) => AeVariant::Int as _,
        Variant::TaggedInt(_) => AeVariant::IntList as _,
        Variant::Vector2(_) => AeVariant::Point as _,
        Variant::Bool(_) => AeVariant::Bool as _,
        Variant::Color(_) => AeVariant::Color as _,
        Variant::Image(_) => AeVariant::Image as _,
    };

    ParamIdx::Dynamic((index as i32 * PARAM_TYPE_COUNT) + STATIC_PARAMS_OFFSET + variant)
}

pub fn update_param_defaults_and_labels(
    state: &mut crate::PluginState,
    local: &mut crate::instance::Instance,
) -> Result<(), ae::Error> {
    for (i, (name, var)) in local.runner.iter_inputs().enumerate() {
        let index = as_param_index(i, var);
        let mut def = state.params.get_mut(index)?;
        match def.as_param_mut()? {
            ae::Param::CheckBox(mut cb) => {
                if let Variant::Bool(b) = var {
                    cb.set_default(b.default);
                    cb.set_value(b.current);
                }
            }
            ae::Param::Color(mut co) => {
                if let Variant::Color(c) = var {
                    let val = c.default;
                    co.set_default(PF_Pixel {
                        alpha: (val[3] * 255.0) as u8,
                        red: (val[0] * 255.0) as u8,
                        green: (val[1] * 255.0) as u8,
                        blue: (val[2] * 255.0) as u8,
                    });

                    let val = c.current;
                    co.set_value(PF_Pixel {
                        alpha: (val[3] * 255.0) as u8,
                        red: (val[0] * 255.0) as u8,
                        green: (val[1] * 255.0) as u8,
                        blue: (val[2] * 255.0) as u8,
                    });
                }
            }
            ae::Param::FloatSlider(mut fl) => {
                if let Variant::Float(f) = var {
                    fl.set_default(f.default as f64);
                    fl.set_value(f.current as f64);
                    fl.set_valid_min(f.min);
                    fl.set_valid_max(f.max);
                    fl.set_slider_min(f.min);
                    fl.set_slider_max(f.max);
                }
            }
            ae::Param::Point(mut p) => {
                if let Variant::Vector2(pt) = var {
                    p.set_default(pt.default.into());
                    p.set_value(pt.current.into());
                }
            }
            ae::Param::Popup(mut il) => {
                if let Variant::TaggedInt(v) = var {
                    il.set_value(v.value);
                }
            }
            ae::Param::Slider(mut i) => {
                if let Variant::Int(v) = var {
                    i.set_default(v.default);
                    i.set_value(v.current);
                    i.set_valid_min(v.min);
                    i.set_valid_max(v.max);
                    i.set_slider_min(v.min);
                    i.set_slider_max(v.max);
                }
            }
            ae::Param::Layer(mut im) => {
                im.set_default_to_this_layer();
            }
            _ => {}
        }

        def.set_name(name);
        def.update_param_ui()?;
        def.set_value_changed();
    }

    Ok(())
}

fn set_debug_vis(state: &mut crate::PluginState, visible: bool) -> Result<(), ae::Error> {
    for idx in ParamIdx::DebugGroupBegin.idx()..=ParamIdx::DebugGroupEnd.idx() {
        set_param_visibility(state.in_data, ParamIdx::from(idx as usize), visible)?;
    }
    Ok(())
}

fn set_script_vis(
    state: &mut crate::PluginState,
    script_loaded: bool,
    venv_loaded: bool,
) -> Result<(), ae::Error> {
    set_param_visibility(state.in_data, ParamIdx::LoadButton, !script_loaded)?;
    set_param_visibility(state.in_data, ParamIdx::ReloadButton, script_loaded)?;
    set_param_visibility(state.in_data, ParamIdx::UnloadButton, script_loaded)?;
    set_param_visibility(state.in_data, ParamIdx::SetVenv, !venv_loaded)?;
    set_param_visibility(state.in_data, ParamIdx::UnsetVenv, venv_loaded)?;
    Ok(())
}

fn set_sequential_control_vis(
    state: &mut crate::PluginState,
    is_sequential: bool,
    // none if no render active
    render_progress: Option<f32>,
) -> Result<(), ae::Error> {
    set_param_visibility(
        state.in_data,
        ParamIdx::ContinuousRenderGroupBegin,
        is_sequential,
    )?;

    set_param_visibility(
        state.in_data,
        ParamIdx::StartRender,
        is_sequential && render_progress.is_none(),
    )?;

    set_param_visibility(
        state.in_data,
        ParamIdx::CancelRender,
        is_sequential && render_progress.is_some(),
    )?;

    set_param_visibility(
        state.in_data,
        ParamIdx::ContinuousRenderGroupEnd,
        is_sequential,
    )?;

    let label = match render_progress {
        Some(prog) => {
            format!("Cancel: %{:.2}", prog)
        }
        None => String::from("Cancel"),
    };

    let mut prog = state.params.get_mut(ParamIdx::CancelRender)?;
    prog.set_name(&label);

    let param_util_suite = ae::pf::suites::ParamUtils::new()?;

    param_util_suite.update_param_ui(
        state.in_data.effect(),
        ParamIdx::CancelRender.idx(),
        &prog,
    )?;

    Ok(())
}

fn set_user_param_vis(
    state: &mut crate::PluginState,
    local: &mut crate::instance::Instance,
    script_loaded: bool,
) -> Result<(), ae::Error> {
    for index in ParamIdx::IsImageFilter.idx()..ParamIdx::ParametersEnd.idx() {
        set_param_visibility(state.in_data, ParamIdx::from(index as usize), false)?;
    }

    let params = local.runner.iter_inputs().enumerate();
    let params_exist = params.size_hint().0 != 0;

    set_param_visibility(
        state.in_data,
        ParamIdx::ParametersStart,
        script_loaded && params_exist,
    )?;

    for (i, (_, var)) in local.runner.iter_inputs().enumerate() {
        let index = as_param_index(i, var);
        set_param_visibility(state.in_data, index, true)?;
    }

    let first_image_input = local
        .runner
        .iter_inputs()
        .enumerate()
        .find(|(_, (_, v))| matches!(v, Variant::Image(_)));

    // only show image filter options IF we have at least one image input
    set_param_visibility(
        state.in_data,
        ParamIdx::IsImageFilter,
        first_image_input.is_some(),
    )?;

    // Toggle first image visibility if we are no longer a filter
    if let Some((i, (_, var))) = first_image_input {
        let index = as_param_index(i, var);

        let is_image_filter = state
            .params
            .get(ParamIdx::IsImageFilter)?
            .as_checkbox()?
            .value();

        set_param_visibility(state.in_data, index, !is_image_filter)?;
    }
    Ok(())
}

pub fn update_input_visibilities(
    state: &mut crate::PluginState,
    local: &mut crate::instance::Instance,
) -> Result<(), ae::Error> {
    let script_loaded = local.src.is_some();
    let venv_loaded = local.venv_path.is_some();
    let is_sequential = local.runner.is_sequential();
    let render_progress = local.job_id.and_then(|id| state.global.render_progress(id));

    set_debug_vis(state, script_loaded)?;
    set_script_vis(state, script_loaded, venv_loaded)?;
    set_sequential_control_vis(state, is_sequential, render_progress)?;
    set_user_param_vis(state, local, script_loaded)?;
    Ok(())
}

pub fn set_params(
    in_data: &ae::InData,
    runner: &mut golob_lib::PythonRunner,
) -> Result<Vec<(String, ParamIdx)>, Error> {
    let curr = in_data.current_time();
    let step = in_data.time_step();
    let scale = in_data.time_scale();
    let mut first_image = true;

    runner.set_time(curr as f32 / scale as f32);
    let mut out = vec![];

    for (i, (name, val)) in runner.iter_inputs_mut().enumerate() {
        let index = as_param_index(i, &*val);

        let param = ae::ParamDef::checkout(*in_data, index.idx(), curr, step, scale, None)?;

        let is_image_filter = ae::ParamDef::checkout(
            *in_data,
            ParamIdx::IsImageFilter.idx(),
            curr,
            step,
            scale,
            None,
        )?
        .as_checkbox()?
        .value();

        match val {
            Variant::Image(_) => {
                if first_image && is_image_filter {
                    first_image = false;
                    out.push((name.clone(), INPUT_LAYER_CHECKOUT_ID));
                } else if param.as_layer()?.value().is_some() {
                    out.push((name.clone(), index));
                }
            }
            Variant::Bool(b) => {
                let cb = param.as_checkbox()?;
                b.current = cb.value();
            }
            Variant::TaggedInt(i) => {
                let popup = param.as_popup()?;
                i.value = popup.value();
            }
            Variant::Color(c) => {
                let color = param.as_color()?;
                let val = color.value();
                c.current = [
                    val.red as f32 / 255.0,
                    val.green as f32 / 255.0,
                    val.blue as f32 / 255.0,
                    val.alpha as f32 / 255.0,
                ];
            }
            Variant::Int(i) => {
                let int = param.as_slider()?;
                i.current = int.value();
            }
            Variant::Float(f) => {
                let float = param.as_float_slider()?;
                f.current = float.value() as f32;
            }
            Variant::Vector2(p) => {
                let vec = param.as_point()?;
                p.current = vec.value().into();
            }
        }
    }

    Ok(out)
}

fn static_params_cfg() -> ParamFlag {
    ParamFlag::CANNOT_TIME_VARY
        | ParamFlag::TWIRLY
        | ParamFlag::SUPERVISE
        | ParamFlag::SKIP_REVEAL_WHEN_UNHIDDEN
}

// set up the params that every instance uses
pub fn setup_static_params(params: &mut ae::Parameters<ParamIdx>) -> Result<(), Error> {
    params.add_group(
        ParamIdx::ScriptGroupStart,
        ParamIdx::ScriptGroupEnd,
        "Script Management",
        |params| {
            params
                .add_with_flags(
                    ParamIdx::LoadButton,
                    "Select Script",
                    ae::ButtonDef::setup(|f| {
                        f.set_label("Select Script");
                    }),
                    static_params_cfg(),
                    ae::ParamUIFlags::empty(),
                )
                .unwrap();

            params.add_with_flags(
                ParamIdx::UnloadButton,
                "Unload Script",
                ae::ButtonDef::setup(|f| {
                    f.set_label("Unload Script");
                }),
                static_params_cfg(),
                ae::ParamUIFlags::empty(),
            )?;

            params
                .add_with_flags(
                    ParamIdx::SetVenv,
                    "Set Site Packages Path",
                    ae::ButtonDef::setup(|f| {
                        f.set_label("Set Site Packages Path");
                    }),
                    static_params_cfg(),
                    ae::ParamUIFlags::empty(),
                )
                .unwrap();

            params.add_with_flags(
                ParamIdx::SetVenv,
                "Unset Site Packages Path",
                ae::ButtonDef::setup(|f| {
                    f.set_label("Unset Site Packages Path");
                }),
                ParamFlag::CANNOT_TIME_VARY
                    | ParamFlag::TWIRLY
                    | ParamFlag::SUPERVISE
                    | ParamFlag::SKIP_REVEAL_WHEN_UNHIDDEN,
                ae::ParamUIFlags::empty(),
            )?;

            params.add_with_flags(
                ParamIdx::ReloadButton,
                "Reload Script",
                ae::ButtonDef::setup(|f| {
                    f.set_label("Reload Script");
                }),
                static_params_cfg(),
                ae::ParamUIFlags::empty(),
            )?;

            Ok(())
        },
    )?;

    params.add_group(
        ParamIdx::DebugGroupBegin,
        ParamIdx::DebugGroupEnd,
        "Debug",
        |params| {
            params.add_with_flags(
                ParamIdx::ShowDebug,
                "Show Debug",
                ae::CheckBoxDef::setup(|f| {
                    f.set_label("Show Debug");
                    f.set_default(true);
                }),
                static_params_cfg(),
                ae::ParamUIFlags::empty(),
            )?;

            params.add_with_flags(
                ParamIdx::DebugOffset,
                "Log UI Offset",
                ae::PointDef::setup(|f| {
                    f.set_default((0.0, 0.0));
                }),
                static_params_cfg(),
                ae::ParamUIFlags::empty(),
            )?;

            params.add_with_flags(
                ParamIdx::TemporalWindow,
                "Approx Temporal Window",
                ae::SliderDef::setup(|f| {
                    f.set_default(1);
                    f.set_valid_min(1);
                    f.set_slider_min(1);
                    f.set_slider_max(20);
                    f.set_valid_max(20);
                }),
                static_params_cfg(),
                ae::ParamUIFlags::empty(),
            )?;
            Ok(())
        },
    )?;

    params.add_group(
        ParamIdx::ContinuousRenderGroupBegin,
        ParamIdx::ContinuousRenderGroupEnd,
        "Sequential Mode",
        |params| {
            params.add_with_flags(
                ParamIdx::StartRender,
                "Start Render",
                ae::ButtonDef::setup(|f| {
                    f.set_label("Start Render");
                }),
                static_params_cfg(),
                ae::ParamUIFlags::empty(),
            )?;

            params.add_with_flags(
                ParamIdx::CancelRender,
                "Cancel Render",
                ae::ButtonDef::setup(|f| {
                    f.set_label("Cancel Render");
                }),
                static_params_cfg(),
                ae::ParamUIFlags::empty(),
            )?;

            Ok(())
        },
    )?;

    Ok(())
}

// create one param of every type to back
// a single input variant in the render context
pub fn create_variant_backing(params: &mut ae::Parameters<ParamIdx>) -> Result<(), Error> {
    let mut base_index = STATIC_PARAMS_OFFSET;
    params.add_group(
        ParamIdx::ParametersStart,
        ParamIdx::ParametersEnd,
        "User Parameters",
        |params| {
            params.add_with_flags(
                ParamIdx::IsImageFilter,
                "Is Image Filter",
                ae::CheckBoxDef::setup(|f| {
                    f.set_label("Enabled");
                    f.set_default(true);
                }),
                ParamFlag::CANNOT_TIME_VARY
                    | ParamFlag::TWIRLY
                    | ParamFlag::SKIP_REVEAL_WHEN_UNHIDDEN,
                ae::ParamUIFlags::empty(),
            )?;

            for _ in 0..MAX_INPUTS {
                for offset in 0..PARAM_TYPE_COUNT {
                    let name = format!("INPUT {}", base_index + offset);
                    let index = ParamIdx::Dynamic(base_index + offset);
                    let ui_flags = ae::ParamUIFlags::empty();
                    let param_flag = ParamFlag::TWIRLY | ParamFlag::SKIP_REVEAL_WHEN_UNHIDDEN;
                    match offset as usize {
                        f if f == AeVariant::Float as usize => params.add_with_flags(
                            index,
                            &name,
                            ae::FloatSliderDef::setup(float),
                            param_flag,
                            ui_flags,
                        )?,
                        i if i == AeVariant::Int as usize => params.add_with_flags(
                            index,
                            &name,
                            ae::SliderDef::setup(int),
                            param_flag,
                            ui_flags,
                        )?,
                        i if i == AeVariant::IntList as usize => params.add_with_flags(
                            index,
                            &name,
                            ae::PopupDef::setup(options),
                            param_flag,
                            ui_flags,
                        )?,
                        pt if pt == AeVariant::Point as usize => params.add_with_flags(
                            index,
                            &name,
                            ae::PointDef::setup(point),
                            param_flag,
                            ui_flags,
                        )?,
                        b if b == AeVariant::Bool as usize => params.add_with_flags(
                            index,
                            &name,
                            ae::CheckBoxDef::setup(bool),
                            param_flag,
                            ui_flags,
                        )?,
                        c if c == AeVariant::Color as usize => params.add_with_flags(
                            index,
                            &name,
                            ae::ColorDef::setup(color),
                            param_flag,
                            ui_flags,
                        )?,
                        i if i == AeVariant::Image as usize => params.add_with_flags(
                            index,
                            &name,
                            ae::LayerDef::setup(layer),
                            param_flag,
                            ui_flags,
                        )?,
                        _ => {
                            unreachable!("Arithmetic mistake while setting up input unions.")
                        }
                    }
                }

                base_index += PARAM_TYPE_COUNT;
            }

            Ok(())
        },
    )?;

    Ok(())
}

/// This is get values to pass to background thread renders
/// it pulls data from the API meant for making panel plugins.
pub fn set_variant_from_stream_val(
    variant: &mut Variant,
    stream_val: StreamValue,
) -> Result<(), Error> {
    match (variant, stream_val) {
        (Variant::Image(_), StreamValue::LayerId(_)) => {}
        (Variant::Bool(val), StreamValue::OneD(fl)) => val.current = fl == 1.0,
        (Variant::TaggedInt(options), StreamValue::OneD(opt)) => options.value = opt as _,
        (Variant::Int(val), StreamValue::OneD(fl)) => val.current = fl as i32,
        (Variant::Float(val), StreamValue::OneD(fl)) => val.current = fl as f32,
        (Variant::Vector2(val), StreamValue::TwoD { x, y }) => val.current = [x as f32, y as f32],
        (
            Variant::Color(val),
            StreamValue::Color {
                alpha,
                red,
                green,
                blue,
            },
        ) => {
            val.current = [red as f32, green as f32, blue as f32, alpha as f32];
        }
        (v, s) => {
            log::error!("Mismatched variant and stream value: {:?} {:?}", v, s);
            return Err(Error::Generic);
        }
    };
    Ok(())
}

pub fn set_param_visibility(in_data: InData, index: ParamIdx, visible: bool) -> Result<(), Error> {
    let dyn_stream_suite = suites::DynamicStream::new()?;
    let stream_suite = suites::Stream::new()?;
    let interface = suites::PFInterface::new()?;

    let effect =
        interface.new_effect_for_effect(in_data.effect(), *crate::PLUGIN_ID.get().unwrap())?;
    let stream = stream_suite.new_effect_stream_by_index(
        effect,
        *crate::PLUGIN_ID.get().unwrap(),
        index.idx(),
    )?;
    dyn_stream_suite.set_dynamic_stream_flag(
        stream,
        DynamicStreamFlags::Hidden,
        false,
        !visible,
    )?;

    Ok(())
}

fn layer(_f: &mut ae::LayerDef) {}

fn color(f: &mut ae::ColorDef) {
    f.set_default(ae::Pixel8 {
        alpha: 255,
        red: 255,
        blue: 255,
        green: 255,
    });
}

fn point(f: &mut ae::PointDef) {
    f.set_default((0.0, 0.0));
}

fn bool(f: &mut ae::CheckBoxDef) {
    f.set_label("Enabled");
    f.set_default(false);
}

fn options(f: &mut ae::PopupDef) {
    // it is unsafe to dynamically set options
    // But from reading the adobe forums it *seems* like it
    // is okay IFF you set the num options constant, so I pad with "-"
    f.set_options(&["option"; 8]);
    f.set_default(0);
}

fn int(f: &mut ae::SliderDef) {
    f.set_default(0);
    f.set_valid_min(-10_000);
    f.set_valid_max(10_000);
    f.set_slider_min(-100);
    f.set_slider_max(100);
}

fn float(f: &mut ae::FloatSliderDef) {
    f.set_default(0.);
    f.set_valid_min(-10_000.);
    f.set_valid_max(10_000.);
    f.set_slider_min(0.0);
    f.set_slider_max(1.0);
    f.set_precision(2);
}
