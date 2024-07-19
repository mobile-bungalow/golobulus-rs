## Golobulus 

#### Python image processing embedded in rust, embedded in after effects.

Warning! This is a very experimental tool! Use at your own risk and the risk of your projects.

Golobulus allows the user to write arbitrary python 3.12 and run it in after effects to produce visual effects. For instance, here is a simple pass through effect:

```python 
import numpy as np

def setup(ctx):
    ctx.register_image_input("input")


def run(ctx):
    input = ctx.get_input("input")

    if input is None:
        return

    ctx.set_output_size(input.shape[0], input.shape[1])
    output = ctx.output()

    np.copyto(output, input)

```

Just define a module with a synchronous or async `run` function and a one time `setup` function and apply the script to a layer in after effects. See the examples directory for my demonstrations of how to leverage the API.

Don't have after effects? Don't want to get near it? Golobulus has a tool `golob_playground` for hot reloading scripts that you can play with in order to build tools to distribute to your friends, or just hack around in a visual python environment without the hassle of using a GUI toolkit or a browser based notebook.

---

### Installation
  
#### Windows
  1. Unzip the `Golobulus.zip` archive downloaded from the release section of the repo. 
  2. Drag the entire unzipped folder into the after effects plugin directory. Likely "C:\Program Files\Adobe\Common\Plug-ins\7.0\MediaCore"

#### MacOs
  currently only silicon Macs are supported.
  1. Unzip the `Golobulus.plugin.zip` archive downloaded from the release section of the repo. 
  2. Drag the `Golobulus.plugin` into the after effects plugin directory. 
  
If you want to configure the version of python installed, or have a preexisting python installation you would like to use globally instead of downloading the *rather large* dynamic library standalone distribution. I recommend following the build instructions below, if you build in a venv or similar environment, the resulting library will link to and respect your system environment and installed packages. 

---

### API Reference, v 0.1

This API is subject to change.

### Module structure

Each Golobulus effect must have two functions, it can maintain state within reason and import any code your system can run natively. It ships with a fully bundled and self contained version of python 3.12, so your less technically savvy friends can drag and drop it into their plugins folder and run your credible and trustworthy code.

#### `setup(ctx: Context)`
  The setup function runs once when initializing the module, you can use this to cache state. This function must be synchronous. When running in after effects it is called on the main thread, so it will lock up the UI for the duration of the call. It is automatically passed a registration context object, which can be used to specify inputs, output dimensions, and retrieve information about the available drawing space and plugin version.

```python
# The minimum required setup code
def setup(ctx):
    pass
```

#### `run(ctx: Context)`
  The run function will be called on the render threads in after effects repeatedly, it can be synchronous or asynchronous. You can interact with the output of this function by calling the `get_output` function on the context to retrieve a mutable numpy array, Note that this does not imply true parallellism because the GIL still applies, but does allow for some multitasking on IO bound or batched tasks. 

```python
# The minimum required run code, will simply zero the output image
def run(ctx):
    pass

# alternatively
async def run(ctx):
    pass
```

#### `Context`

  The context object encapsulates the core API for interacting with after effects, you can use it to specify up to 32 inputs of various types which can be keyframed or manipulated with script.

#### `output() -> np.array`
  returns a mutable numpy array with `dtype` respecting the current bit depth of the after effects project, its is in RGBA channel order unless specified by calling `set_automatic_color_correction` with `False`, in which case it will return the output array in ARGB order, after effects native format. This array is only valid during the `run` call when it is passed, accessing it outside of that function will likely result in a crash.

#### `max_output_size() -> (integer, integer)`
  returns the maximum allowable output (height, width) pair. This corresponds to the layer size in pixels.

#### `set_output_size(height: integer, width: integer)`
  If specified all subsequent calls to `get_output`, in this and future calls to `run`, will return a subarray blitting to the direct center of the output layer with the requested dimensions. 
### Exceptions:
this will throw a runtime exception if any dimensions are requested at 0 or below, or if the requested image exceeds the available dimensions of the output layer. 

#### `register_image_input(name: string)`
  *only valid in setup*
  Specifies a layer input on the effect, when selected by the user it will be passed in as an immutable numpy array. Note that only the first image input can be used to acquire the pixels of the layer the effect is applied to.

```python
def setup(ctx):
    ctx.register_image_input("input")

def run(ctx):
    # numpy array available in run, None if the user has not set it.
    input = ctx.get_input("input")
```

#### `register_int(name: string, min: integer = -100, max: integer = 100, default: integer = 0 )`
  *only valid in setup*
  Specifies an integer input which can be keyframed from After Effects, accessible in `run`.

#### `register_float(name: string, min: float= -100.0, max: float = 100.0, default: float = 0.0 )`
  *only valid in setup*
  Specifies an float input which can be keyframed from After Effects, accessible in `run`.

#### `register_bool(name: string, default: bool = false)`
  *only valid in setup*
  Specifies an bool input, data made accessible in `run`.

#### `register_point(name: string, min: float[2] = [-100.0, -100.0] max: float[2] = [100.0, 100.0], default: float[2] = [0.0, 0.0] )`
  *only valid in setup*
  Specifies a bounded 2d point input, data made accessible in `run`.

#### `register_color(name: string, default: float[4] = [1.0, 1.0, 1.0, 1.0])`
  *only valid in setup*
  Specifies a color input, data made accessible in `run`. normalized floating point rgba.

#### `get_input(name: string) -> Any`
  Returns the input specified in `setup` under name with a value keyframed by the user.

#### `set_automatic_color_correction(on: bool)`
  *only valid in setup*
 defaults to True, this makes the API take slightly longer to swizzle input and output images to and from ARGB format, which is after effects native channel ordering. If you are okay with a couple milliseconds overhead don't bother with this flag.

#### `set_sequential_mode(on: bool)`
  *only valid in setup*
  if `True` is passed, the effect will run as a pass through layer *however* there will be a button available for the user to begin a background thread render which guarantees that frames are rendered serially. When the process is complete the result will be stored as an image sequence and inserted into the users project filling up the current active region. This is useful for scripts which are noninteractively slow. sequential renders always happen in RGBA channel ordering and always at the maximum resolution possible for your composition, the output respects the color depth of your project.

#### `is_sequential_mode() -> bool`
 returns `True` if the effect is running in sequential mode, `False` otherwise.

#### `time() -> float`
  Returns the local comp time in seconds.

#### `build_info() -> string`
  Returns a version string.

---

### Building

You will need [just](https://github.com/casey/just) installed, which can be done with `cargo install just`.

Download the after effects sdk for your desired platform and point to it with the `AESDK_ROOT` environment variable. You may also need an appropriate version of MSVC with Clang 16.0 + installed on windows, and development tools with a signing certificate set up on MacOs.

On MacOs and Windows you can build with the following command from the root directory of this repo.

```bash
just -f golob_plugin/Justfile build
```

Debug builds will link the system python, they will respect the current venv and will be functional so long as numpy is installed.

You can build a self contained release with

```Bash
just -f golob_plugin/Justgile release
```

### Future Directions
- CI/CD, need to self host mac aarch64, x86, and Windows containers.
- Add a way to generate a preview frame for sequential mode.
- Automatically detect venv / sites package if relative to the project file.
- Structured json output - much like sequential mode allow the insertion of JSON layers so your scripts can create keyframes from image analysis.
- Shape layers / paths inputs.
- Gpu buffer mode, Use something like cupy to let users specify they are a zero copy gpu effect.
- Layer time offsets - Allow for specifying multiple samples of images from the same layer, but at different times.
- Tracking point api.



