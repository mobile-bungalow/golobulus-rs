def setup(ctx):
    ctx.register_int("foo", default=0, min=0, max=255)
    pass


def run(ctx):
    output = ctx.get_output()
    foo = ctx.get_input("foo")
    output.fill(foo)
