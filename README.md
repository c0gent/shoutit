ShoutIt
=======

A message broadcasting app that uses the
[OneSignal](https://about.onesignal.com/) push notification delivery network.


### What does it do?

The ShoutIt client allows the user to send text broadcasts to all users with
the app installed. Simply type a message and press the "Shout It!" button.


### How does it work?

The client (currently only available for Android) sends an HTTP POST to the
server with a JSON encoded body containing the users message. That message is
parsed by the server then relayed to the OneSignal REST API. From there,
OneSignal's delivery network will push the message out to all users with the
client installed.