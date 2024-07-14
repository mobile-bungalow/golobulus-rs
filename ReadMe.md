## Globulus (working title)

Python image processing embedded in rust, embedded in after effects.

This repository consists of three parts, a library: golob_lib, an application: golob_playground, and an after effects plugin: golob_plugin. The library embeds a python context in rust which can execute arbitrary code. 

That means a small snippet such as this:

```python
import cairo
import numpy
import torch

def setup():
  # your one time setup

def run():
  # your image processing, called on every render


```

...Can be orchestrated in rust like this: 

```rust

let mut runner = PythonRunner::default();
runner.load_script(EMPTY, None)?;

// Load your image data here
let input = vec![0u8; 255 * 255 * 4];

// Prepare your output buffer here
let mut output = vec![0u8; 255 * 255 * 4];

let i = InDesc {
    fmt: ImageFormat::Rgba8,
    data: &input,
    width: 255,
    height: 255,
};

let o = OutDesc {
    fmt: ImageFormat::Rgba8,
    data: &mut output,
    width: 255,
    height: 255,
};

// Please note that a single input and output are just for testing purposes
runner.run(i, o)?

```
### Project Name

Named after the absurd snake hybrid antagonist from G.I. Joe.
