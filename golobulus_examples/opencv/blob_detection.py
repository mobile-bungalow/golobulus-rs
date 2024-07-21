import cv2
import numpy as np
from PIL import Image

blob = None
last_params = None
output_array = None


def setup(ctx):
    ctx.register_image_input("image")
    ctx.register_float("min_blob_size", min=1.0, max=100.0, default=10.0)
    ctx.register_float("max_blob_size", min=1.0, max=500.0, default=100.0)
    ctx.register_float("min_circularity", min=0.0, max=1.0, default=0.3)
    ctx.register_float("min_convexity", min=0.0, max=1.0, default=0.87)
    ctx.register_float("min_inertia_ratio", min=0.0, max=1.0, default=0.01)
    ctx.register_color("square_color", default=[1.0, 0.0, 0.0, 1.0])  # Default red


def create_blob_detector(params):
    detector_params = cv2.SimpleBlobDetector_Params()
    detector_params.minThreshold = 10
    detector_params.maxThreshold = 200
    detector_params.filterByArea = True
    detector_params.minArea = params["min_blob_size"] ** 2
    detector_params.maxArea = params["max_blob_size"] ** 2
    detector_params.filterByCircularity = True
    detector_params.minCircularity = params["min_circularity"]
    detector_params.filterByConvexity = True
    detector_params.minConvexity = params["min_convexity"]
    detector_params.filterByInertia = True
    detector_params.minInertiaRatio = params["min_inertia_ratio"]
    return cv2.SimpleBlobDetector_create(detector_params)


def run(ctx):
    global blob, last_params, output_array

    input_image = ctx.get_input("image")
    if input_image is None:
        return

    ctx.set_output_size(input_image.shape[0], input_image.shape[1])
    output = ctx.output()

    params = {
        "min_blob_size": ctx.get_input("min_blob_size"),
        "max_blob_size": ctx.get_input("max_blob_size"),
        "min_circularity": ctx.get_input("min_circularity"),
        "min_convexity": ctx.get_input("min_convexity"),
        "min_inertia_ratio": ctx.get_input("min_inertia_ratio"),
    }

    if last_params != params:
        blob = create_blob_detector(params)
        last_params = params.copy()

    col = ctx.get_input("square_color")
    square_color = (
        int(col[0] * 255),
        int(col[1] * 255),
        int(col[2] * 255),
        int(col[3] * 255),
    )

    gray = cv2.cvtColor(input_image, cv2.COLOR_RGB2GRAY)
    keypoints = blob.detect(gray)

    if output_array is None or output_array.shape != input_image.shape:
        output_array = np.array(input_image)
    else:
        np.copyto(output_array, input_image)

    for kp in keypoints:
        x, y = int(kp.pt[0]), int(kp.pt[1])
        size = int(kp.size)
        half_size = size // 2
        cv2.rectangle(
            output_array,
            (x - half_size, y - half_size),
            (x + half_size, y + half_size),
            square_color,
            2,
        )

    output[...] = output_array
