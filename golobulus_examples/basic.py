import numpy as np
import cairo


def setup(ctx):
    ctx.register_float("foo", default=0, min=0, max=1)
    pass


def run(ctx):
    output = ctx.get_output()
    foo = ctx.get_input("foo")
    (w, h) = (output.shape[0], output.shape[1])
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, w, h)
    cairo_ctx = cairo.Context(surface)

    cairo_ctx.move_to(w / 2, h / 3)
    cairo_ctx.line_to(2 * w / 3, 2 * h / 3)
    cairo_ctx.rel_line_to(-1 * w / 3, 0)
    cairo_ctx.close_path()
    cairo_ctx.set_source_rgb(np.sin(ctx.time()), foo, 1.0)
    cairo_ctx.set_line_width(15)
    cairo_ctx.stroke()
    cairo_out = np.ndarray(shape=(w, h, 4), dtype=np.uint8, buffer=surface.get_data())

    np.copyto(output, cairo_out)
