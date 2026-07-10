"""Planted fixture: Thumbnail's constructor is invoked from another file
(ctor_caller.py), not called as a bare function — constructor calls are
call-related to __init__, not just plain function calls (see callrel.rs's
constructor-call handling).
"""


class Thumbnail:
    """A generated image thumbnail plus its render bookkeeping."""

    def __init__(self, image_id, width, height, quality=85, image_format="jpeg"):
        self.image_id = image_id
        self.width = width
        self.height = height
        self.quality = quality
        self.image_format = image_format
        self.render_count = 0
        self.last_error = None
        self.cache_key = f"{image_id}:{width}x{height}:{image_format}"
