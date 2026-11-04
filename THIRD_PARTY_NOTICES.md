# Third-Party Notices

Meetily itself is licensed under the MIT License (see `LICENSE.md`). This file lists
third-party components added by the optional VibeVoice-ASR integration
(`vibevoice-server/`). They are not bundled into the desktop app; the sidecar installs them
into its own virtualenv.

## VibeVoice-ASR

- Project: https://github.com/microsoft/VibeVoice — model card https://huggingface.co/microsoft/VibeVoice-ASR
- Quantized MLX weights used by default: https://huggingface.co/mlx-community/VibeVoice-ASR-4bit
- Copyright (c) Microsoft Corporation.
- License: MIT

```
MIT License

Copyright (c) Microsoft Corporation.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## mlx-audio

- Project: https://github.com/Blaizzy/mlx-audio
- Copyright (c) Prince Canuma and contributors.
- License: MIT (same terms as above)

## MLX

- Project: https://github.com/ml-explore/mlx
- Copyright (c) Apple Inc.
- License: MIT (same terms as above)

## Python service dependencies

FastAPI (MIT), Uvicorn (BSD-3-Clause), python-multipart (Apache-2.0), NumPy (BSD-3-Clause).
FFmpeg is invoked as an external program and is not distributed (LGPL/GPL depending on build).
