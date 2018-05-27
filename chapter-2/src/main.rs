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

//! # Chapter 2
//!
//! This chapter introduces two new concepts coming from libp2p: connection upgrades, and the
//! swarm.
//!
//! Libp2p lets you *upgrade* an open connection to a protocol. Once dialing or listening succeeds,
//! instead of a socket you will obtain the *output* of the upgrade.
//! The nature of this output depends on the upgrade you apply and can be various things, such as a
//! stream that wraps around the socket, various information about the remote, a future that must
//! be driven to completion, etc.

extern crate futures;
extern crate libp2p;
extern crate rand;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_stdin;

use futures::{Future, Stream};
use tokio_core::reactor::Core;

use libp2p::core::Transport;
use libp2p::floodsub::{FloodSubController, FloodSubUpgrade, TopicBuilder};
use libp2p::{Multiaddr, PeerId};

fn main() {
    // Same as in chapter 1.
    let mut core = Core::new().unwrap();
    let transport = libp2p::tcp::TcpConfig::new(core.handle());

    let listen_multiaddr: Multiaddr = "/ip4/0.0.0.0/tcp/0"
        .parse()
        .expect("failed to parse multiaddress");

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

    // Let's use the swarm to listen, instead of the raw transport.
    let actual_multiaddr = swarm_controller
        .listen_on(listen_multiaddr)
        .expect("failed to listen");
    println!("Now listening on {}", actual_multiaddr);

    // Now let's handle the floodsub protocol.
    // We already have `floodsub_rx`, which was created earlier. It is a `Stream` of all the
    // messages that we receive from connections upgraded with `floodsub_upgrade`.
    // In order to use floodsub, we also need to create a `FloodSubController`.
    let floodsub_controller = FloodSubController::new(&floodsub_upgrade);

    // All the messages dispatched through the floodsub protocol belong to what is called a
    // *topic*. This is what we create here.
    let topic = TopicBuilder::new("workshop-chapter2-topic").build();

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

    // *** WORKSHOP ACTION ITEM HERE ***
    //
    // Your task in this chapter is to write the rest of the program:
    //
    // - Iterate over `std::env::args().skip(1)` and call `swarm.dial()` to dial the
    //   address.
    // - Use the `tokio-stdin` crate to read the message written on stdin, and publish each
    //   message on the network by calling `floodsub_controller.publish()`.
    //
    // Once you manage to get two nodes to talk to each other, you can spawn a third node.
    // Connect nodes B and C to A, and notice how messages sent by B or C get relayed to C or B
    // by going through A.
    //

    for peer in std::env::args().skip(1) {
        swarm_controller
            .dial(
                peer.parse().expect("Argument is not a valid multiaddress"),
                upgr_trans_with_muxing.clone(),
            )
            .expect("Failed to connect to Peer");
    }

    // This is a place-holder. Thanks to the `tokio-stdin` crate, create a stream that produces
    // the messages obtained from stdin, and call `for_each()` on it to obtain a future.
    let mut buf = Vec::new();
    let stdin_future = tokio_stdin::spawn_stdin_stream_unbounded()
        .map_err(|()| std::io::Error::new(std::io::ErrorKind::Other, "Could not open io stream."))
        .for_each(|byte| {
            buf.push(byte);
            if byte == b'\n' {
                floodsub_controller.publish(&topic, buf.clone());
                buf.clear();
            }
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
    core.run(final_future).unwrap();
}
