def center_crop(image, crop_height, crop_width):
    h, w, _ = image.shape
    start_y = (h - crop_height) // 2
    start_x = (w - crop_width) // 2

    return image[start_y : start_y + crop_height, start_x : start_x + crop_width, :]


def rgba_view(arr):
    return arr[..., [1, 2, 3, 0]].view()


def swizzle_in_place(arr):
    arr[:] = arr[..., [3, 0, 1, 2]]
