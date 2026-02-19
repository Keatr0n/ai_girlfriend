# ai_girlfriend

Again, the name is a joke. I just wanted to see if I could use rust to wire up Whisper, an LLM and a TTS engine (Piper in this instance) to create a local version of the voice call that chat gpt and a lot of the other big ai companies have.

Turns out with the power of ai, anything is possible. (A fair amount of this was written by Claude and ol' gipity tho that didn't stop me from learning from this experience.)

Anyway, as for models, I'm using [Qwen3 4B Instruct 2507](https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-FP8) for the LLM because it's pretty quick and good with functions.

This uses llama.cpp under the hood, so you'll need a gguf model. Sadly no safetensors here.

And Whisper I'm using just [ggml-base with a Q8 quant](https://huggingface.co/ggerganov/whisper.cpp/tree/main) as it runs a little better on my cpu.

As for the Piper voice, that's something I'm keeping to myself 😉 ([sourced from here tho](https://brycebeattie.com/files/tts/))

## Running

If you're normal, you may need [FFmpeg](https://www.ffmpeg.org/) and [Piper](https://github.com/OHF-Voice/piper1-gpl) for the command calls.

You may need to change Cargo.toml for your specific flavour of gpu acceleration (in my case ROCm) but it also supports CUDA as well as Metal if you plan on running this on the greats inference machine ever made (the mac mini)

Then just run

```shell
cargo run
```

---

If you like pain and snow flakes, it'll all be in the flake

```shell
nix develop
```

then

```
cargo run
```

## Commands

There are a few keyboard shortcuts that can help when she misunderstands you or your mum walks into the room

`m`: Will **m**ute the mic when it's listening.

`t`: Will enter **t**ext mode so you can send messages without using your mic.

`↑`: (up arrow) Will allow you to edit the last message sent.

`esc`: (escape key) Will cancel inference and delete the last message or cancel the current edit (down arrow also works for this).

`?`: (question mark) Will show thinking tags if you are using a thinking model.

`n`: Will set the model to only respond after you say it's **n**ame.

## Configuration and Customization

Before you start, you'll need to fill in the `config.toml`

Under `[global]` you'll find all your standard things, like model locations and llm config stuff.

And under `[[assistant]]` you can set up and customize your many girlfrie... I mean assistants. There is an example one included so you know what options you have, but the only things that are required are a name and system prompt.

## Tools
There is also a rudimentary tool support. If you supply a tool_path that points to a python file, it can use any top level function in that file when required. (Some version of python must be installed for this) You can also set individual tool files per assistant too. It will also pass in the docstring for context to the llm, so it's recommended you add one.

For safety, you can only use str, int, float, and bool arguments for the functions. That being said this can still be extremely dangerous and can lave you open to prompt injection attacks among other things.

So please be mindful with what you give the llm access to. And add as many guardrails as you can. For instance, if you are giving it write access to a certain part of the filesystem, make sure you block all attempts to traverse up with `../`.

I have tried to ensure as much safety as I can, but I am not responsible if you decide to give it full terminal access and it wants to pull a `sudo rm -rf / --no-preserve-root` on you.
