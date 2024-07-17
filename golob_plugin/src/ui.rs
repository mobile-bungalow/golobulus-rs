use after_effects as ae;
use after_effects::drawbot;
use after_effects_sys::{DRAWBOT_PointF32, DRAWBOT_RectF32};

pub fn draw(
    in_data: &ae::InData,
    local: &mut crate::instance::Instance,
    params: &mut ae::Parameters<crate::ParamIdx>,
    event: &mut ae::EventExtra,
) -> Result<(), ae::Error> {
    let ev = event.event();

    match ev {
        ae::Event::Draw(_) => draw_text(
            in_data,
            params,
            &local.timestamped_error_log,
            &local.timestamped_log,
            event,
        ),
        _ => Ok(()),
    }
}

fn draw_text(
    in_data: &ae::InData,
    params: &mut ae::Parameters<crate::ParamIdx>,
    error_map: &std::collections::HashMap<i32, String>,
    log_map: &std::collections::HashMap<i32, String>,
    event: &mut ae::EventExtra,
) -> Result<(), ae::Error> {
    let show_debug = params
        .get(crate::ParamIdx::ShowDebug)?
        .as_checkbox()?
        .value();

    let offset = params
        .get(crate::ParamIdx::DebugOffset)?
        .as_point()?
        .value();

    let window = params
        .get(crate::ParamIdx::TemporalWindow)?
        .as_slider()?
        .value()
        - 1;

    if !show_debug {
        event.set_event_out_flags(ae::EventOutFlags::HANDLED_EVENT);
        return Ok(());
    }

    let drawbot = event.context_handle().drawing_reference()?;
    let supplier = drawbot.supplier()?;
    let surface = drawbot.surface()?;
    let error_color = drawbot::ColorRgba {
        alpha: 1.0,
        blue: 0.02,
        green: 0.02,
        red: 0.9,
    };
    let std_color = drawbot::ColorRgba {
        alpha: 1.0,
        blue: 0.92,
        green: 0.92,
        red: 0.92,
    };
    let backing_color = drawbot::ColorRgba {
        alpha: 1.0,
        blue: 0.05,
        green: 0.05,
        red: 0.05,
    };

    let tf = layer_to_frame_tf(in_data, event)?;
    let original_size = supplier.default_font_size()?;
    let font_width = (tf.mat[0][0].powi(2) + tf.mat[1][1].powi(2)).sqrt() * original_size;
    let font = supplier.new_default_font(font_width)?;

    let mut pt = ae::sys::PF_FixedPoint {
        x: ae::Fixed::from(offset.0).as_fixed(),
        y: ae::Fixed::from(offset.1).as_fixed(),
    };

    event
        .callbacks()
        .layer_to_comp(in_data.current_time(), in_data.time_scale(), &mut pt)?;

    event.callbacks().source_to_frame(&mut pt)?;

    let time = in_data.current_time();
    let step = in_data.time_step();
    let local_frame = in_data.current_frame_local();

    let draw_messages = |map: &std::collections::HashMap<i32, String>,
                         color: &drawbot::ColorRgba,
                         offset: &mut f32,
                         label: &str|
     -> Result<(), ae::Error> {
        let brush = supplier.new_brush(color)?;

        let offset_output = (-window..=window).filter_map(|i| {
            let t = time + (i * step);
            map.get(&t).map(|v| (i * step, v))
        });

        for (time_offset, string) in offset_output {
            let cur_frame = (time_offset / step) + local_frame as i32;
            let formatted_string = format!("[frame:{cur_frame} - {label}] {string}");
            // Draw string only does one line at a time
            for line in formatted_string.lines() {
                surface.paint_rect(
                    &backing_color,
                    &DRAWBOT_RectF32 {
                        left: ae::Fixed::from_fixed(pt.x).as_f32(),
                        // pad bottom and top 7.5%
                        top: ae::Fixed::from_fixed(pt.y).as_f32() + ((*offset - 0.85) * font_width),
                        // There are no monospace fonts and no layout information
                        // so ad hoc background boxes it is. 0.6 em is the monospaced AR so
                        // it's a fair guess.
                        // FIXME: find any principled way to draw this background rect.
                        width: (line.len() as f32 * font_width * 0.6),
                        height: font_width,
                    },
                )?;

                surface.draw_string(
                    &brush,
                    &font,
                    line,
                    &DRAWBOT_PointF32 {
                        x: ae::Fixed::from_fixed(pt.x).as_f32(),
                        y: ae::Fixed::from_fixed(pt.y).as_f32() + (*offset * font_width),
                    },
                    drawbot::TextAlignment::Left,
                    drawbot::TextTruncation::None,
                    0.0,
                )?;
                *offset += 1.0;
            }
        }
        Ok(())
    };

    let mut offset = 0.0;
    draw_messages(error_map, &error_color, &mut offset, "err")?;
    draw_messages(log_map, &std_color, &mut offset, "stdout")?;

    event.set_event_out_flags(ae::EventOutFlags::HANDLED_EVENT);
    Ok(())
}

fn layer_to_frame_tf(
    in_data: &ae::InData,
    event: &mut ae::EventExtra,
) -> Result<ae::drawbot::MatrixF32, ae::Error> {
    // construct the unit axes and then transform them to get the
    // matrix we use for displaying our ad hoc UI
    let mut pts = [
        ae::sys::PF_FixedPoint {
            x: ae::Fixed::from(0.0).as_fixed(),
            y: ae::Fixed::from(0.0).as_fixed(),
        },
        ae::sys::PF_FixedPoint {
            x: ae::Fixed::from(1.0).as_fixed(),
            y: ae::Fixed::from(0.0).as_fixed(),
        },
        ae::sys::PF_FixedPoint {
            x: ae::Fixed::from(0.0).as_fixed(),
            y: ae::Fixed::from(1.0).as_fixed(),
        },
    ];

    for pt in &mut pts {
        event
            .callbacks()
            .layer_to_comp(in_data.current_time(), in_data.time_scale(), pt)?;

        event.callbacks().source_to_frame(pt)?;
    }

    Ok(after_effects_sys::DRAWBOT_MatrixF32 {
        mat: [
            [
                ae::Fixed::from_fixed(pts[1].x - pts[0].x).as_f32() as _,
                ae::Fixed::from_fixed(pts[1].y - pts[0].y).as_f32() as _,
                0.0,
            ],
            [
                ae::Fixed::from_fixed(pts[2].x - pts[0].x).as_f32() as _,
                ae::Fixed::from_fixed(pts[2].y - pts[0].y).as_f32() as _,
                0.0,
            ],
            [
                ae::Fixed::from_fixed(pts[0].x).as_f32() as _,
                ae::Fixed::from_fixed(pts[0].y).as_f32() as _,
                1.0,
            ],
        ],
    })
}
