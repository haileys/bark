# <img src="https://user-images.githubusercontent.com/179065/260457176-b0975ce3-03a0-4df8-a979-a2ba84b3b039.png" width=64 height=51> bark!

low latency multi-receiver synchronised audio streaming for local networks.

* Transmits uncompressed 48khz stereo audio over UDP multicast

### Running the server under Pipewire or Pulse

Note: if using Pipewire, you must have `pipewire-alsa` installed for this to work.

* First create a virtual node for Bark to receive audio from. You will configure applications to send audio to this node.

    ```sh-session
    $ pactl load-module module-null-sink media.class=Audio/Duplex sink_name=Bark audio.position=FL,FR
    ```

* You can list all sources on your system with `pactl`:

    ```sh-session
    $ pactl list sources short
    145     Bark    PipeWire        float32le 2ch 48000Hz   SUSPENDED
    3676    alsa_output.usb-Focusrite_Scarlett_Solo_USB-00.analog-stereo.monitor     PipeWire        s32le 2ch 44100Hz       IDLE
    3677    alsa_input.usb-Focusrite_Scarlett_Solo_USB-00.analog-stereo      PipeWire        s32le 2ch 44100Hz       SUSPENDED
    3678    alsa_input.usb-046d_Logitech_Webcam_C930e-02.analog-stereo     PipeWire        s16le 2ch 44100Hz       SUSPENDED
    ```

* Run the Bark server passing the name of the sink you created with the `--device` option:

    ```sh-session
    $ bark stream --multicast 224.100.100.100:1530 --device Bark
    ```

### Running the receiver

* Find the sink you want the receiver to output to:

    ```sh-session
    $ pactl list sinks short
    145     Bark    PipeWire        float32le 2ch 48000Hz   SUSPENDED
    3676    alsa_output.usb-Focusrite_Scarlett_Solo_USB-00.analog-stereo     PipeWire        s32le 2ch 44100Hz       RUNNING
    ```

* Run the Bark receiver:

    ```sh-session
    $ bark receive --multicast 224.100.100.100:1530 --device alsa_output.usb-Focusrite_Scarlett_Solo_USB-00.analog-stereo
    ```

### Configuration

As well as on the command line, Bark's options can be set by environment variable or configuration file. Command line options and their corresponding environment variables are shown in `bark --help`.

Bark also searches the XDG config directories for a `bark.toml` configuration file, respecting any custom directories set in `XDG_CONFIG_DIRS`.

By default, Bark will look in `$HOME/.config/bark.toml` first, and then `/etc/bark.toml`. Options set in the configuration file take lowest precedence, are overriden by environment variables, and then finally command line options take highest precedence.

The config file supports all command line options Bark supports. Here's an example:

```toml
multicast = "224.100.100.100:1530"

[source]
device = "Bark"
delay_ms = 15

[receive]
device = "alsa_output.usb-Focusrite_Scarlett_Solo_USB-00.analog-stereo"
```

### Monitoring the stream

Run `bark stats` to see a live view of the state of all Bark receivers.

Four timing fields are shown for each receiver:

* **Audio:** The time offset of the audio stream, from when it should be according to the stream presentation timestamp, to when the receiver is actually playing. A positive offset means the receiver is _ahead_ of the stream, a negative offset means the receiver is _behind_ the stream.

* **Buffer:** The length of the audio data in the receiver buffer.

* **Network:** The one-way packet delay between stream source and receiver. For best sync, this should be as stable as possible.

* **Predict:** The offset from the data timestamp in an audio packet (the stream source's time when the packet was sent), to what the receiver thinks the data timestamp should be according to measured clock difference and network latency.

### Tuning

The stream source is responsible for setting the delay of the audio stream. The delay wants to be as low as possible without causing receivers to slew or underrun their buffers too much. Receivers will always experience _some_ slewing to keep in sync - the network is not perfectly reliable, and clocks always run at slightly different rates - but ideally slewing should be kept to a minimum to ensure best quality. Keep an eye on `bark stats` while tuning this value.

The optimal delay value depends on your network, particularly with respect to packet loss and latency stability (receivers connecting wirelessly will need more delay to remain stable than those hard-wired), as well as the latency introduced by sound cards. I've observed that my desktop, which has a USB DAC, consistently tends to have less in its buffer than receivers with PCI DACs.
