#![crate_name = "tcp_simple"]
#![crate_type="lib"]

use std::thread;
use std::io::{Read, Write};
use std::sync::mpsc::channel;
use std::collections::HashMap;
use std::sync::mpsc::{Sender, Receiver};
use std::net::{TcpListener, TcpStream, Shutdown, SocketAddrV4, SocketAddr};

pub struct Message {
  count: usize,
  buf: [u8; 1400],
}

pub struct TcpCfg {
  address: SocketAddrV4,
  max_conn: usize,
}

pub struct TcpSock {
  conns: HashMap<SocketAddrV4, TcpStream>,
  listener: TcpListener,
  msg_rx: Receiver<Message>,
  conn_rx: Receiver<(SocketAddrV4, TcpStream)>,
  disc_rx: Receiver<SocketAddrV4>,
}

pub enum TcpErr {
  NotConnected,
  IoErr(std::io::Error),
}

pub enum TcpEvent {
  RecvMsg(Message),
  NewConn(SocketAddrV4),
  Disconn(SocketAddrV4),
  Nil,
}

impl TcpSock {
  pub fn new_from_cfg(cfg: TcpCfg) -> TcpSock {
    let conns = HashMap::new();
    let listener = TcpListener::bind(cfg.address).unwrap();

    let (conn_tx, conn_rx) = channel();
    let (disc_tx, disc_rx) = channel();
    let (recv_tx, recv_rx) = channel();
    let accept_listener = listener.try_clone().unwrap();
    let sock = TcpSock{
      conns: conns,
      listener: listener,
      msg_rx: recv_rx,
      conn_rx: conn_rx,
      disc_rx: disc_rx,
    };

    thread::spawn(move|| {
      TcpSock::accept_loop(conn_tx, recv_tx.clone(), &accept_listener);
    });

    sock
  }

  pub fn close(self) {
    drop(self.listener);
  }

  pub fn receive(&mut self) -> Option<Message> {
    match self.msg_rx.try_recv() {
      Ok(m) => Some(m),
      Err(_) => None,
    }
  }

  fn recv_loop(recv_tx: Sender<Message>, mut conn: TcpStream) {
    let mut buf = [0; 1400];
    loop {
      match conn.read(&mut buf) {
        Ok(count) if count > 0 => {
          match recv_tx.send(Message{count: count, buf: buf}) {
            Ok(_) => {},
            Err(_) => break, 
          }
        },
        _ => break,
      }
    }
    let _ = conn.shutdown(Shutdown::Both);
  }

  fn accept_loop(conn_tx: Sender<(SocketAddrV4, TcpStream)>, recv_tx: Sender<Message>, listener: &TcpListener) {
    for stream in listener.incoming() {
      match stream {
        Ok(stream) => {
          match stream.try_clone() {
            Ok(c) => {
              let rtx = recv_tx.clone();
              thread::spawn(move|| {
                TcpSock::recv_loop(rtx, c);
              });
            },
            Err(_) => { continue; },
          }
          match stream.peer_addr().unwrap() {
            SocketAddr::V4(s) => {
              match conn_tx.send((s, stream)) {
                Ok(_) => {},
                Err(_) => {},
              }
            }
            _ => {}
          }
        },
        Err(_) => continue, 
      };
    }
  }

  pub fn send(&mut self, to: &SocketAddrV4, buf: &[u8]) -> Result<usize, TcpErr> {
    match self.conns.get_mut(&to) {
      Some(conn) => match conn.write(buf) {
        Ok(s) => Ok(s),
        Err(e) => Err(TcpErr::IoErr(e)),
      },
      _ => Err(TcpErr::NotConnected)
    }
  }

  pub fn send_to_all(&mut self, buf: &[u8]) {
    for (_, conn) in self.conns.iter_mut() {
      let _ = conn.write(buf);
    }
  }

  pub fn poll(&mut self) -> TcpEvent {
    match self.conn_rx.try_recv() {
      Ok((addr, conn)) => {
        self.conns.insert(addr, conn);
        return TcpEvent::NewConn(addr);
      },
      Err(_) => {},
    };

    match self.msg_rx.try_recv() {
      Ok(msg) => return TcpEvent::RecvMsg(msg),
      Err(_) => {},
    };

    TcpEvent::Nil
  }

  pub fn disconnect(&mut self, disc: &SocketAddrV4) -> Result<(), TcpErr> {
    match self.conns.get_mut(&disc) {
      Some(conn) => match conn.shutdown(Shutdown::Both) {
        Ok(()) => {},
        Err(e) => { return Err(TcpErr::IoErr(e)); },
      },
      _ => { return Err(TcpErr::NotConnected); },
    };
    self.conns.remove(disc).unwrap();
    Ok(())
  }
}
