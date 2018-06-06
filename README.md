ShoutIt
=======

A very basic message broadcasting app that uses the
[OneSignal](https://about.onesignal.com/) push notification delivery network.


### Installation and Usage

Clone this repo, open the `shoutit/client-android/OneSignalShoutIt/` folder in
Android Studio, then press the Run App button.


### What does it do?

The ShoutIt client allows the user to send text broadcasts to all users with
the app installed. Simply type a message and press the "Shout It!" button.


### How does it work?

The client (currently only available for Android) sends an HTTP POST to the
server with a JSON encoded body containing the users message. That message is
parsed by the server then relayed asynchronously to the OneSignal REST API.
From there, OneSignal's delivery network will push the message out to all
users with the client installed.

The client uses [Volley](https://github.com/google/volley) and the [OneSignal
Android SDK](https://github.com/OneSignal/OneSignal-Android-SDK).

The server uses [hyper](https://github.com/hyperium/hyper) to send messages
asynchronously using the [futures](https://github.com/rust-lang-nursery/futures-rs) API.


#### Problems?

This is an experimental for-fun project but please feel free to report any
problems or ask questions on the
[Issues](https://github.com/c0gent/shoutit/issues) page.
