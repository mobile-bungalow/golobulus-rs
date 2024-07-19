import numpy as np
import sys
from scipy import ndimage


def setup(ctx):
    print(sys.path)
    ctx.register_image_input("input")


def run(ctx):
    input = ctx.get_input("input")
    if input is None:
        return

    grayscale = np.dot(input[..., :3], [0.2989, 0.5870, 0.1140])
    sobel_x = ndimage.sobel(grayscale, axis=0, mode="reflect")

    ctx.set_output_size(input.shape[0], input.shape[1])
    output = ctx.output()

    output[...] = sobel_x[..., np.newaxis]
