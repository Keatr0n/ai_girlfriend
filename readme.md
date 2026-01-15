# ai_girlfriend
Again, the name is a joke. I just wanted to see if I could use rust to wire up Whisper, an LLM and a TTS engine (Piper in this instance) to create a local version of the voice call that chat gpt and a lot of the other big ai companies have.

Turns out with the power of ai, anything is possible.

A fair amount of this was written by Claud and ol' gipity tho that didn't stop me from learning from this experience.

Anyway, as for models, I'm using LFM2 8B Q8 for the LLM because I have slow ram and no gpu acceleration (thanks nixos + integrated graphics).
And Whisper I'm using just ggml-base with a Q8 quant as it runs a little better on my cpu.
As for the Piper voice, that's something I'm keeping to myself 😉

## Running

If you're normal
```shell
cargo run
```
---
If you like pain and snow flakes
```shell
nix develop
```
then 
```
cargo run
```
