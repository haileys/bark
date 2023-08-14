# bark!

low latency multi-receiver synchronised audio streaming for local networks.

* Transmits uncompressed PCM data over UDP multicast

* Relies on synchronised system clocks. Recommended implementation is to run a [chronyd](https://wiki.archlinux.org/title/Chrony) server locally, it can achieve precision in the tens of microseconds over LANs


### Running the server under Pipewire

* First create a virtual node for Bark to receive audio from. You will configure applications to send audio to this node.

    ```sh-session
    $ pactl load-module module-null-sink media.class=Audio/Duplex sink_name=Bark audio.position=FL,FR
    ```

* Then find the Pipewire node ID for the new virtual node you just created. You can use `pactl` and `jq` to do this programatically. The output on my machine looks something like:

    ```sh-session
    $ pactl --format=json list sources | jq 'map({ key: .name, value: .index }) | from_entries'
    {
      "alsa_output.usb-Focusrite_Scarlett_Solo_USB-00.analog-stereo.monitor": 75,
      "alsa_input.usb-Focusrite_Scarlett_Solo_USB-00.analog-stereo": 76,
      "alsa_input.usb-Logitech_Webcam_C930e-02.analog-stereo": 77,
      "Bark": 145
    }
    ```

* Run the Bark server passing the node ID in the `PIPEWIRE_NODE` environment variable:

    ```sh-session
    $ PIPEWIRE_NODE=145 bark stream --group 224.100.100.100 --port 1234
    ```

* Then run your audio source! You can use the same node ID to arrange for it to send its audio to the Bark node:

    ```sh-session
    $ PIPEWIRE_NODE=145 ffplay music.flac
    ```
