// Copyright 2018 Pierre Krieger
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! # Chapter 3
//!
//! The goal of this chapter is to take the code of chapter 2 and make it compile and run inside
//! of a browser!
//!
//! In order to run this code in the browser, follow these steps:
//!
//! - Install docker if you haven't done so yet.
//! - Create a docker container with the image `tomaka/rustc-emscripten`. This can be done by
//!   running `docker run --rm -it -v `pwd`:/usr/code -w /usr/code tomaka/rustc-emscripten` from
//!   the root of this repository.
//! - From inside the container, go to the `chapter-3` directory and run
//!   `cargo build --target=asmjs-unknown-emscripten`.
//! - Open the `browser.html` file included in this crate in your browser. It should automatically
//!   find the generated JavaScript code.
//!
//! In addition to `browser.html`, you are also given a file `platform.rs`. This file contains
//! platform-independant code that allows you to run an events loop and receive messages from stdin
//! in a cross-plaform way. See the usage in the `main()` function below.
//!
//! The browser doesn't support dialing to a TCP port. The only protocol that is allowed is
//! websockets. Good news, however! The `build_transport()` method in the `platform` module
//! automatically builds a transport that supports websockets. To use them, instead of dialing
//! `/ip4/1.2.3.4/tcp/1000`, you can dial `/ip4/1.2.3.4/tcp/1000/ws`.
//!
//! Additionally, please note that the browser doesn't support listening on any connection (even
//! websockets). Calling `listen_on` will trigger an error at runtime. You can use
//! `if cfg!(not(target_os = "emscripten")) { ... }` to listen only when outside of the browser.
//!
//! Good luck!

extern crate futures;
extern crate libp2p;
extern crate rand;
extern crate tokio_io;
extern crate tokio_stdin;

use futures::{Future, Stream};

use libp2p::core::Transport;
use libp2p::floodsub::{FloodSubController, FloodSubUpgrade, TopicBuilder};
use libp2p::{Multiaddr, PeerId};

#[cfg(target_os = "emscripten")]
#[macro_use]
extern crate stdweb;

mod platform;

fn main() {
    // The `PlatformSpecific` object allows you to handle the transport and stdin in a
    // cross-platform manner.
    let platform = platform::PlatformSpecific::default();

    // This builds an implementation of the `Transport` trait (similar to the `TcpConfig` object in
    // earlier chapters).
    let transport = platform.build_transport();

    // This builds a stream of messages coming from stdin.
    let stdin = platform.stdin();

    // Insert your code here!

    // We are going to tweak `transport` so that all the incoming and outgoing connections
    // automatically negotiate a protocol named *floodsub*. Floodsub is a pub-sub protocol that
    // allows one to propagate messages throughout the network.
    // Tweaking the transport is done by first creating a `FloodSubUpgrade`, then calling
    // `with_upgrade`.
    //
    // As part of the protocol, which need to pass a *PeerId* to `FloodSubUpgrade::news()`. In this
    // workshop we just generate it randomly.
    let (floodsub_upgrade, floodsub_rx) = {
        let key = (0..2048).map(|_| rand::random::<u8>()).collect::<Vec<_>>();
        FloodSubUpgrade::new(PeerId::from_public_key(&key))
    };
    let upgraded_transport = transport.with_upgrade(floodsub_upgrade.clone());

    // We now create a *swarm*. A swarm is a convenient object that is responsible for handling all
    // the incoming and outgoing connections in a single point.
    // In other words, instead of using the transport to listen and dial, we will use the swarm.
    //
    // The `swarm` function requires not just a `Transport` but a `MuxedTransport`. *Muxing*
    // consists in making multiple streams go through the same socket so that we don't need to open
    // a new connection every time. In order to add support for muxing with any transport, we can
    // just call the `with_dummy_muxing()` method of the `Transport` trait.
    let upgr_trans_with_muxing = upgraded_transport.with_dummy_muxing();
    let (swarm_controller, swarm_future) =
        libp2p::swarm(upgr_trans_with_muxing.clone(), |future, _remote_addr| {
            // The first parameter of this closure (`future`) is the output of the floodsub
            // upgrade. If we didn't apply any upgrade on the transport, it would be the raw socket
            // instead.
            //
            // In the case of floodsub, the output is a future that must be driven to completion
            // for the protocol to work.
            // Coincidentially, the return value of this closure must be a future that is going to
            // be integrated inside of `swarm_future`. By driving `swarm_future` to completion, we
            // will also drive to completion the future coming from floodsub.
            future
        });

    if cfg!(not(target_os = "emscripten")) {
        let listen_multiaddr: Multiaddr = "/ip4/0.0.0.0/tcp/63204/ws"
            .parse()
            .expect("failed to parse multiaddress");
        // Let's use the swarm to listen, instead of the raw transport.
        let actual_multiaddr = swarm_controller
            .listen_on(listen_multiaddr)
            .expect("failed to listen");
        println!("Now listening on {}", actual_multiaddr);
    }

    // Now let's handle the floodsub protocol.
    // We already have `floodsub_rx`, which was created earlier. It is a `Stream` of all the
    // messages that we receive from connections upgraded with `floodsub_upgrade`.
    // In order to use floodsub, we also need to create a `FloodSubController`.
    let floodsub_controller = FloodSubController::new(&floodsub_upgrade);

    // All the messages dispatched through the floodsub protocol belong to what is called a
    // *topic*. This is what we create here.
    let topic = TopicBuilder::new("workshop-chapter3-topic").build();

    // We need to subscribe to a topic in order to receive the messages that belong to it.
    // Subscribing to a topic broadcasts a message over the network to signal all the connected
    // nodes that we are interested in this topic.
    floodsub_controller.subscribe(&topic);

    // Let's tweak `floodsub_rx` so that we print on stdout the messages we receive.
    let floodsub_rx = floodsub_rx.for_each(|msg| {
        if let Ok(msg) = String::from_utf8(msg.data) {
            println!("> {}", msg);
        } else {
            println!("Received non-utf8 message");
        }

        Ok(())
    });

    let args: Vec<String> = if cfg!(not(target_os = "emscripten")) {
        std::env::args().skip(1).collect()
    } else {
        vec!["/ip4/127.0.0.1/tcp/63204/ws".to_owned()]
    };

    for peer in args {
        swarm_controller
            .dial(
                peer.parse().expect("Argument is not a valid multiaddress"),
                upgr_trans_with_muxing.clone(),
            )
            .expect("Failed to connect to Peer");
    }

    let stdin_future = stdin.for_each(move |message| {
        floodsub_controller.publish(&topic, message.into_bytes());
        Ok(())
    });

    // `final_future` is a future that contains all the behaviour that we want, but nothing has
    // actually started yet. Because we created the `TcpConfig` with tokio, we need to run the
    // future through the tokio core.
    let final_future = swarm_future
        .select(floodsub_rx)
        .map_err(|(err, _)| err)
        .and_then(|(_, n)| n)
        .select(stdin_future)
        .map_err(|(err, _)| err)
        .and_then(|(_, n)| n);
    // core.run(final_future).unwrap();

    // Instead of `core.run()`, use `platform.run()`.
    platform.run(final_future);
}
