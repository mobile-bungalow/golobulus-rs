import numpy as np
import torch
from transformers import pipeline
from PIL import Image

if torch.backends.mps.is_available():
    device = "mps"
elif torch.cuda.is_available():
    device = "cuda"
else:
    device = "cpu"

pipe = pipeline(
    task="depth-estimation",
    model="depth-anything/Depth-Anything-V2-Small-hf",
    device=device,
)


def setup(ctx):
    ctx.register_image_input("image")
    pass


def run(ctx):
    input = ctx.get_input("image")

    if input is None:
        return

    ctx.set_output_size(input.shape[0], input.shape[1])
    output = ctx.output()

    im = Image.fromarray(input)
    depth = pipe(im)["depth"]
    # arr = numpy.asarray(depth)
    # output[..., :3] = arr[..., numpy.newaxis]
    # output[..., 3] = 255
