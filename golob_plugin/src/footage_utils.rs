use crate::{background_task, PLUGIN_ID};
use after_effects as ae;
use golob_lib::ImageFormat;
use image::{error::ImageError, ImageBuffer, Rgba};
use std::path::PathBuf;

pub struct FootageImportTask {
    pub comp_handle: ae::aegp::CompHandle,
    pub layer_index: usize,
    pub path: PathBuf,
    pub insertion_time: after_effects::Time,
}

impl FootageImportTask {
    pub fn new(in_data: &ae::InData, output_path: PathBuf) -> Result<Self, ae::Error> {
        let pf_interface = ae::aegp::suites::PFInterface::new()?;
        let layer_suite = ae::aegp::suites::Layer::new()?;
        let comp_suite = ae::aegp::suites::Comp::new()?;

        let this_layer = pf_interface.effect_layer(in_data.effect())?;
        let comp_handle = layer_suite.layer_parent_comp(this_layer)?;
        let layer_index = layer_suite.layer_index(this_layer)?;
        let work_area_start = comp_suite.comp_work_area_start(comp_handle)?;

        Ok(Self {
            comp_handle,
            layer_index,
            path: output_path,
            insertion_time: work_area_start,
        })
    }
}

pub fn get_layer_pixels(
    shared_buffer: &mut background_task::ImageBuffer,
    layer_handle: &ae::aegp::LayerHandle,
    time: ae::Time,
) -> Result<(), ae::Error> {
    let render_suite = ae::aegp::suites::Render::new()?;
    let render_options_suite = ae::aegp::suites::LayerRenderOptions::new()?;
    let world_suite = ae::aegp::suites::World::new()?;

    log::debug!("{}", line!());
    let layer_render_options =
        render_options_suite.new_from_layer(layer_handle, *PLUGIN_ID.get().unwrap())?;

    log::debug!("{}", line!());
    let opts = ae::aegp::LayerRenderOptions::from_handle(layer_render_options, false);
    opts.set_time(time)?;

    log::debug!("{}", line!());
    let receipt = render_suite.render_and_checkout_layer_frame::<fn() -> bool>(opts, None)?;
    let world_handle = render_suite.receipt_world(receipt)?;

    log::debug!("{}", line!());
    let world = ae::aegp::World::from_handle(world_handle, false);
    let mut real_layer: after_effects_sys::PF_LayerDef = unsafe { std::mem::zeroed() };

    log::debug!("{}", line!());
    world_suite.fill_out_pf_effect_world(world, &mut real_layer)?;
    log::debug!("{}", line!());

    let layer = ae::Layer::from_aegp_world(std::ptr::null(), world_handle)?;
    log::debug!("{}", line!());

    let format = match layer.bit_depth() {
        8 => golob_lib::ImageFormat::Argb8,
        16 => golob_lib::ImageFormat::Argb16ae,
        _ => golob_lib::ImageFormat::Argb32,
    };

    let data = layer.buffer();

    if shared_buffer.data.len() != data.len() {
        shared_buffer.data = data.to_owned();
    } else {
        shared_buffer.data.copy_from_slice(data);
    }

    shared_buffer.width = real_layer.width as u32;
    shared_buffer.height = real_layer.height as u32;
    shared_buffer.format = format;
    shared_buffer.stride = layer.buffer_stride() as u32;

    render_suite.checkin_frame(receipt)?;

    log::debug!("{}", line!());
    Ok(())
}

pub fn import_footage(task: FootageImportTask) -> Result<(), ae::Error> {
    let FootageImportTask {
        comp_handle,
        layer_index,
        path,
        insertion_time,
    } = task;

    log::debug!("importing footage {path:?}.");

    let footage_suites = ae::aegp::suites::Footage::new()?;
    let item_suites = ae::aegp::suites::Item::new()?;
    let layer_suite = ae::aegp::suites::Layer::new()?;
    let proj_suites = ae::aegp::suites::Project::new()?;
    let project_handle = proj_suites.project_by_index(0).unwrap();

    let new_footage = footage_suites.new_footage(
        *PLUGIN_ID.get().unwrap(),
        path.to_str().unwrap(),
        None,
        None,
        ae::aegp::InterpretationStyle::DialogOk,
    )?;

    let mut cur = item_suites.first_proj_item(&project_handle)?;
    let mut found = false;

    // linear search for golobulus directory
    while !cur.is_null() {
        let Some(next) = item_suites.next_proj_item(&project_handle, cur)? else {
            break;
        };

        if item_suites.item_name(cur, *PLUGIN_ID.get().unwrap())? == "golobulus"
            && item_suites.item_type(cur)? == ae::aegp::ItemType::Folder
        {
            found = true;
            break;
        }

        cur = next;
    }

    if !found {
        cur = item_suites.create_new_folder("golobulus", None)?;
    }

    let footage = footage_suites.add_footage_to_project(new_footage, &cur)?;
    let new_layer = layer_suite.add_layer(&footage, comp_handle)?;

    layer_suite.reorder_layer(new_layer, layer_index.saturating_sub(1) as i32)?;
    layer_suite.set_layer_offset(new_layer, insertion_time)?;

    log::debug!("import successful.");
    Ok(())
}

/// Gets that project dir if the project is saved
pub fn get_project_dir() -> Option<PathBuf> {
    let proj_suites = ae::aegp::suites::Project::new().ok()?;
    let project_handle = proj_suites.project_by_index(0).unwrap();
    let proj_path = proj_suites.project_path(project_handle).unwrap();

    if proj_path.is_empty() {
        None
    } else {
        Some(PathBuf::from(proj_path))
    }
}

/// Calculate the number of frames inside the region of interest currently selected by the user.
pub fn get_region_of_interest_frame_count(in_data: &ae::InData) -> Result<u32, ae::Error> {
    let pf_interface = ae::aegp::suites::PFInterface::new()?;
    let layer_suite = ae::aegp::suites::Layer::new()?;
    let comp_suite = ae::aegp::suites::Comp::new()?;

    let this_layer = pf_interface.effect_layer(in_data.effect())?;
    let parent_comp = layer_suite.layer_parent_comp(this_layer)?;
    let work_area_dur = comp_suite.comp_work_area_duration(parent_comp)?;

    let frame_count = work_area_dur.value / in_data.time_step();
    Ok(frame_count as u32)
}

/// Get the project bit depth in Golobulus format.
pub fn get_sequence_output_format() -> Result<golob_lib::ImageFormat, ae::Error> {
    let proj_suites = ae::aegp::suites::Project::new()?;
    let project_handle = proj_suites.project_by_index(0).unwrap();
    let bit_depth = proj_suites.project_bit_depth(project_handle)?;

    let fmt = match bit_depth {
        ae::aegp::ProjectBitDepth::BitDepthU8 => golob_lib::ImageFormat::Argb8,
        ae::aegp::ProjectBitDepth::BitDepthU16 => golob_lib::ImageFormat::Argb16ae,
        ae::aegp::ProjectBitDepth::BitDepthF32 => golob_lib::ImageFormat::Argb32,
    };

    Ok(fmt)
}

/// Creates a sanitized output directory name
/// where we can import the rendered footage.
pub fn output_dir_name(
    effect_ref: &ae::EffectHandle,
    mut project_path: PathBuf,
) -> Result<PathBuf, ae::Error> {
    let pf_interface = ae::aegp::suites::PFInterface::new()?;
    let layer_suite = ae::aegp::suites::Layer::new()?;

    let this_layer = pf_interface.effect_layer(effect_ref)?;
    let (name, source) = layer_suite.layer_name(this_layer, *PLUGIN_ID.get().unwrap())?;

    let name = if name.is_empty() {
        if source.is_empty() {
            String::from("golobulus_render")
        } else {
            source
        }
    } else {
        name
    };
    // pretty bad sanitization but should work for now
    let name = name.replace(':', "-");
    let name = name.replace('/', "-");
    let name = name.replace('\\', "-");
    // file name *.aep
    project_path.pop();
    let output_dir = project_path.join(name);
    Ok(output_dir)
}

/// Writes an image to a file with an appropriate format.
/// given its bit depth.
pub fn write_image_to_file(
    mut path: PathBuf,
    image: &[u8],
    width: u32,
    height: u32,
    fmt: ImageFormat,
) -> Result<(), ImageError> {
    match fmt {
        ImageFormat::Rgba8 | ImageFormat::Argb8 => {
            let buf =
                ImageBuffer::<Rgba<u8>, _>::from_raw(width, height, bytemuck::cast_slice(image))
                    .unwrap();
            path.set_extension("png");
            buf.save(path)?;
        }
        ImageFormat::Argb16ae | ImageFormat::Rgba16 => {
            let buf =
                ImageBuffer::<Rgba<u16>, _>::from_raw(width, height, bytemuck::cast_slice(image))
                    .unwrap();
            path.set_extension("png");
            buf.save(path)?;
        }
        ImageFormat::Argb32 | ImageFormat::Rgba32 => {
            let buf =
                ImageBuffer::<Rgba<f32>, _>::from_raw(width, height, bytemuck::cast_slice(image))
                    .unwrap();
            path.set_extension("exr");
            buf.save(path)?;
        }
    };

    Ok(())
}

/// Creates a directory with name `path` or a suffixed number if it already exists.
pub fn create_suffixed_directory(path: &std::path::Path) -> PathBuf {
    let mut suffix = 1;
    let mut new_path = path.to_path_buf();
    while new_path.exists() {
        new_path = path.with_file_name(format!(
            "{}_{}",
            path.file_name().unwrap().to_str().unwrap(),
            suffix
        ));
        suffix += 1;
    }
    std::fs::create_dir(&new_path).unwrap();
    new_path
}
