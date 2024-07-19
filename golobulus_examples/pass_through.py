import numpy as np

# Simple pass through filter, just copies the current layer to the output
# No requirements


def setup(ctx):
    ctx.register_image_input("input")


def run(ctx):
    input = ctx.get_input("input")

    if input is None:
        return

    ctx.set_output_size(input.shape[0], input.shape[1])
    output = ctx.output()

    np.copyto(output, input)
