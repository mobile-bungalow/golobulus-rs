def setup(ctx):
    import sys

    print(sys.path)
    print(np)
    print(np.__version__)
    ctx.register_image_input("input")


def run(ctx):
    input = ctx.get_input("input")
    if input is None:
        return

    grayscale = np.dot(input[..., :3], [0.2989, 0.5870, 0.1140])

    ctx.set_output_size(input.shape[0], input.shape[1])
    output = ctx.output()

    output[..., :3] = grayscale[..., np.newaxis]
    dtype_info = np.iinfo(output.dtype)
    output[..., 3] = dtype_info.max
