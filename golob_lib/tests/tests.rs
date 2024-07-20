use golob_lib::*;
use image::ImageBuffer;
use image::Rgba;

macro_rules! png_pixels {
    ($file_path:literal) => {{
        let png_bytes = include_bytes!($file_path);

        let decoded_image = image::load_from_memory(png_bytes).expect("Failed to decode PNG image");

        let rgba_image: ImageBuffer<Rgba<u8>, Vec<u8>> = decoded_image.to_rgba8();

        rgba_image.into_raw()
    }};
}

// really stupid test
const IDENT: &str = r#"
import sys
import numpy as np

def setup(ctx):
    input = ctx.register_image_input('input')
    pass

def run(ctx):
    input = ctx.get_input('input')
    output = ctx.output()
    np.copyto(output, input)

"#;

#[test]
fn q_ident() {
    let mut runner = PythonRunner::default();

    runner.load_script(IDENT, None).unwrap();

    let input = vec![110u8; 10 * 4];

    let mut output = vec![0u8; 10 * 4];

    let i = InDesc {
        fmt: ImageFormat::Rgba8,
        data: &input,
        width: 1,
        height: 10,
        stride: None,
    };

    let o = OutDesc {
        fmt: ImageFormat::Rgba8,
        data: &mut output,
        width: 1,
        height: 10,
        stride: None,
    };

    let mut pass = runner.create_render_pass(o);
    pass.load_input(i, "input");

    pass.submit().unwrap();
    assert_eq!(input, output);
}

// really stupid *async* test
const ASYNC_IDENT: &str = r"
import numpy as np

def setup(ctx):
    input = ctx.register_image_input('input')
    pass

async def run(ctx):
    input = ctx.get_input('input')
    output = ctx.output()
    np.copyto(output, input)

";

#[test]
fn async_ident() {
    let mut runner = PythonRunner::default();

    runner.load_script(ASYNC_IDENT, None).unwrap();

    let input = vec![110u8; 100 * 100 * 4];

    let mut output = vec![0u8; 100 * 100 * 4];

    let i = InDesc {
        fmt: ImageFormat::Rgba8,
        data: &input,
        width: 100,
        height: 100,
        stride: None,
    };

    let o = OutDesc {
        fmt: ImageFormat::Rgba8,
        data: &mut output,
        width: 100,
        height: 100,
        stride: None,
    };

    let mut pass = runner.create_render_pass(o);
    pass.load_input(i, "input");

    pass.submit().unwrap();
    assert_eq!(input, output);
}

const CHASE_MODS: &str = r"
from local_mods import speaker 
from local_mods.deeper import deepest 
import sys

def setup(ctx):
    pass

def run(ctx):
    speaker.speak()
    deepest.foo()
    pass

";

#[test]
fn local_mods() {
    let mut runner = PythonRunner::default();
    let path = "./tests/resources";
    runner.set_script_parent_directory(std::path::PathBuf::from(path).canonicalize().unwrap());

    runner.load_script(CHASE_MODS, None).unwrap();

    let mut output = vec![0u8; 10 * 4];

    let o = OutDesc {
        fmt: ImageFormat::Rgba8,
        data: &mut output,
        width: 1,
        height: 10,
        stride: None,
    };

    let pass = runner.create_render_pass(o);

    pass.submit().unwrap();
}

#[test]
fn ident_png() {
    let mut runner = PythonRunner::default();

    runner.load_script(IDENT, None).unwrap();

    let input = png_pixels!("./resources/dog.png");

    let mut output = vec![0u8; input.len()];

    let i = InDesc {
        fmt: ImageFormat::Rgba8,
        data: &input,
        width: 1024,
        height: 577,
        stride: None,
    };

    let o = OutDesc {
        fmt: ImageFormat::Rgba8,
        data: &mut output,
        width: 1024,
        height: 577,
        stride: None,
    };

    let mut pass = runner.create_render_pass(o);
    pass.load_input(i, "input");

    pass.submit().unwrap();
    approximately_equivalent(&input, &output);
}

#[test]
fn ident_png_color_depths() {
    let mut runner = PythonRunner::default();

    runner.load_script(IDENT, None).unwrap();

    let png_bytes = include_bytes!("./resources/dog.png");
    let decoded_image = image::load_from_memory(png_bytes).expect("Failed to decode PNG image");

    {
        // 8bit
        let rgba_image: ImageBuffer<Rgba<u8>, Vec<u8>> = decoded_image.to_rgba8();
        let input = rgba_image.into_raw();

        let mut output = vec![0u8; input.len()];

        let i = InDesc {
            fmt: ImageFormat::Rgba8,
            data: &input,
            width: 1024,
            height: 577,
            stride: None,
        };

        let o = OutDesc {
            fmt: ImageFormat::Rgba8,
            data: &mut output,
            width: 1024,
            height: 577,
            stride: None,
        };

        let mut pass = runner.create_render_pass(o);
        pass.load_input(i, "input");

        pass.submit().unwrap();
        approximately_equivalent(&input, &output);
    }

    {
        // 16bit
        let rgba_image: ImageBuffer<Rgba<u16>, Vec<u16>> = decoded_image.to_rgba16();
        let long = rgba_image.into_raw();
        let input = bytemuck::cast_slice(&long);

        let mut output = vec![0u8; input.len()];

        let i = InDesc {
            fmt: ImageFormat::Rgba16,
            data: input,
            width: 1024,
            height: 577,
            stride: None,
        };

        let o = OutDesc {
            fmt: ImageFormat::Rgba16,
            data: &mut output,
            width: 1024,
            height: 577,
            stride: None,
        };

        let mut pass = runner.create_render_pass(o);
        pass.load_input(i, "input");

        pass.submit().unwrap();
        approximately_equivalent(input, &output);
    }

    {
        // fp32
        let rgba_image: ImageBuffer<Rgba<f32>, Vec<f32>> = decoded_image.to_rgba32f();
        let long = rgba_image.into_raw();
        let input = bytemuck::cast_slice(&long);

        let mut output = vec![0u8; input.len()];

        let i = InDesc {
            fmt: ImageFormat::Rgba32,
            data: input,
            width: 1024,
            height: 577,
            stride: None,
        };

        let o = OutDesc {
            fmt: ImageFormat::Rgba32,
            data: &mut output,
            width: 1024,
            height: 577,
            stride: None,
        };

        let mut pass = runner.create_render_pass(o);
        pass.load_input(i, "input");

        pass.submit().unwrap();
        approximately_equivalent(input, &output);
    }
}

const STDOUT: &str = r"

def setup(ctx):
    print('Hello! World.')
    pass

def run(ctx):
    pass

";

#[test]
fn stdout() {
    let mut runner = PythonRunner::default();

    assert_eq!(
        Some("Hello! World.\n".to_owned()),
        runner.load_script(STDOUT, None).unwrap()
    );
}

const SEQ: &str = r#"
import sys
import numpy

def setup(ctx):
    ctx.set_sequential_mode(True)
    pass

def run(ctx):
    pass

"#;

#[test]
fn seq_mode() {
    let mut runner = PythonRunner::default();
    runner.load_script(SEQ, None).unwrap();
    assert!(runner.is_sequential());
}

const NOT_SEQ: &str = r"

def setup(ctx):
    ctx.set_sequential_mode(False)
    pass

def run(ctx):
    pass

";

#[test]
fn not_seq_mode() {
    let mut runner = PythonRunner::default();
    runner.load_script(NOT_SEQ, None).unwrap();
    assert!(!runner.is_sequential());
}

const ERRORS: &str = r"

def setup(ctx):
    pass

def run(ctx):
    print('Hello! World.')
    this_undeclared_variable_will_cause_a_panic
    pass

";

#[test]
fn error_logging() {
    let mut runner = PythonRunner::default();

    runner.load_script(ERRORS, None).unwrap();

    let mut data = vec![0u8; 40 * 40 * 4];
    let o = OutDesc {
        fmt: ImageFormat::Rgba8,
        width: 40,
        data: &mut data,
        height: 40,
        stride: None,
    };

    let pass = runner.create_render_pass(o);
    let error = pass.submit();

    assert!(error.is_err());

    assert!(matches!(
        error,
        Err(GolobulError::RuntimeError {
            stdout: Some(_),
            ..
        }),
    ));
}

const REG_TEST: &str = r"

def setup(ctx):
    ctx.register_float('float 1', default=0, min=-100, max=100)
    ctx.register_float('float 2', min=100, max=1000, default=200)
    ctx.register_image_input('input')
    pass

def run(ctx):
    pass

";

#[test]
fn registry() {
    let mut runner = PythonRunner::default();

    runner.load_script(REG_TEST, None).unwrap();

    {
        let next = runner.iter_inputs().find(|(n, _)| *n == "float 1").unwrap();
        assert_eq!(*next.1, Variant::Float(Cfg::new(0.0, -100.0, 100.0)));

        let next = runner.iter_inputs().find(|(n, _)| *n == "float 2").unwrap();
        assert_eq!(*next.1, Variant::Float(Cfg::new(200.0, 100.0, 1000.0)));

        let next = runner.iter_inputs().find(|(n, _)| *n == "input").unwrap();
        assert_eq!(*next.1, Variant::Image(DiscreteCfg::new(Image::Input)));
    }
}
const SIZE_CONFIG: &str = r"

def setup(ctx):
    ctx.set_output_size(20, 20)
    pass

def run(ctx):
    assert ctx.output().shape == (20, 20, 4)
    pass

";

#[test]
fn size_config() {
    let mut runner = PythonRunner::default();
    runner.load_script(SIZE_CONFIG, None).unwrap();

    assert_eq!(
        runner.requested_output_resize(),
        Some(OutputSize {
            width: 20,
            height: 20
        })
    );

    let mut wrong_out = [0u8; 40 * 40 * 4];

    let o = OutDesc {
        fmt: ImageFormat::Rgba8,
        data: &mut wrong_out,
        width: 40,
        height: 40,
        stride: None,
    };

    let pass = runner.create_render_pass(o);
    pass.submit().unwrap();

    let mut right_out = [0u8; 20 * 20 * 4];

    let o = OutDesc {
        fmt: ImageFormat::Rgba8,
        data: &mut right_out,
        width: 20,
        height: 20,
        stride: None,
    };

    let pass = runner.create_render_pass(o);
    pass.submit().unwrap();
}

const GRAYSCALE: &str = r"
import numpy as np

def setup(ctx):
    ctx.register_image_input('input')

def run(ctx):
    input = ctx.get_input('input')
    output = ctx.output()

    r, g, b, a = input[..., 0], input[..., 1], input[..., 2], input[..., 3]
    grayscale = 0.2989 * r + 0.5870 * g + 0.1140 * b

    output[..., :3] = grayscale[..., np.newaxis]
    output[..., 3] = a
";

#[test]
fn grayscale() {
    let mut runner = PythonRunner::default();

    runner.load_script(GRAYSCALE, None).unwrap();

    let input = png_pixels!("./resources/grayscale/in.png");

    let mut output = vec![0u8; input.len()];

    let i = InDesc {
        fmt: ImageFormat::Rgba8,
        data: &input,
        width: 256,
        height: 256,
        stride: None,
    };

    let o = OutDesc {
        fmt: ImageFormat::Rgba8,
        data: &mut output,
        width: 256,
        height: 256,
        stride: None,
    };

    let mut pass = runner.create_render_pass(o);
    pass.load_input(i, "input");

    pass.submit().unwrap();

    let snapshot = &png_pixels!("./resources/grayscale/out.png");
    assert!(approximately_equivalent(&output, snapshot));
}

#[test]
fn bad_setup() {
    let mut runner = PythonRunner::default();
    assert!(runner.load_script("", None).is_err());

    // missing setup
    let partial = r"
def run():
    pass

        ";

    assert!(runner.load_script(partial, None).is_err());

    // missing setup
    let partial = r"
def setup():
    pass

        ";

    assert!(runner.load_script(partial, None).is_err());
}

fn approximately_equivalent(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .map(|(a, b)| if a < b { b - a } else { a - b })
            .enumerate()
            .all(|(idx, abs_diff)| {
                let pixel = idx / 4;
                if abs_diff > 3 {
                    panic!("images differ at pixel {pixel}")
                } else {
                    true
                }
            })
}

#[allow(dead_code)]
// at some point real snapshotting tests might be worth it, but for now
// these should hold over
fn write_texture_to_png(
    data: &[u8],
    file_path: &str,
    width: u32,
    height: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let texture: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_vec(width, height, data.to_owned())
        .ok_or("Failed to create ImageBuffer")?;

    // Write the texture to a PNG file.
    texture.save_with_format(file_path, image::ImageFormat::Png)?;
    Ok(())
}
