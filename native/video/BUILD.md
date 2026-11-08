# Rebuilding and relinking the LGPL video helper

The application bundle distributes `frames.c` under MIT, the complete matching
`ffmpeg-source.tar.xz`, FFmpeg's LGPL text, and `build.json` with exact configure
flags. The executable accepts source bytes through stdin and emits raw sampled
RGB frames on stdout. It does not use an installed system FFmpeg.

On Apple Silicon macOS with the command-line development tools:

1. Extract `ffmpeg-source.tar.xz` into a writable working directory.
2. In that FFmpeg source directory, run `./configure` with the flags recorded in
   `build.json`, replacing `--prefix` with an absolute writable directory such as
   `/tmp/face-relink-libs`. Run `make -j4` and `make install`.
3. Compile the supplied helper source against those libraries:

```sh
clang -O2 frames.c -I/tmp/face-relink-libs/include -L/tmp/face-relink-libs/lib \
  -lavformat -lavcodec -lswscale -lavutil -lm -lpthread -liconv \
  -o kokin-video-frames
codesign --force --sign - kokin-video-frames
```

You may modify and rebuild the helper and LGPL libraries for your own use.
Preserve the input/output protocol and processing bounds for compatibility with
the parent face worker. Native resource signing and packaging are development
steps, not remote authorization or a restriction on relinking.
