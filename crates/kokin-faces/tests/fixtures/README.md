# Native face acceptance fixtures

These are public-domain US government portraits, used solely to verify local
face processing. Their names are not fed to the recognition model.

- `grace_hopper.jpg`: official US Navy portrait of Rear Admiral Grace Hopper,
  reproduced unchanged from Matplotlib v3.10.7 sample data.
  https://github.com/matplotlib/matplotlib/blob/v3.10.7/lib/matplotlib/mpl-data/sample_data/grace_hopper.jpg
  SHA256 a8ca6d734765703b09728ab47fe59f473d93ae3967fc24c7c0288c3c7adb7130
- `astronaut.png`: NASA portrait of astronaut Eileen Collins, public domain,
  reproduced unchanged from scikit-image v0.19.3.
  https://github.com/scikit-image/scikit-image/blob/v0.19.3/skimage/data/astronaut.png
  SHA256 88431cd9653ccd539741b555fb0a46b61558b301d4110412b5bc28b5e3ea6cb5
  Dataset attribution: https://scikit-image.org/docs/stable/api/skimage.data.html#skimage.data.astronaut

The acceptance test constructs resized, multi-person, rotated, and blank images
in memory. No generated variants are presented as independent identity samples.
These two portraits are a functional smoke test, not population-level accuracy
or bias validation.
