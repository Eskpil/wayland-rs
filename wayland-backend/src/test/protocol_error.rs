use std::{
    ffi::CString,
    os::unix::prelude::{FromRawFd, IntoRawFd},
    sync::{Arc, Mutex},
};

use crate::rs::socket::{BufferedSocket, Socket};

use super::*;

struct ServerData<Id>(Arc<Mutex<Option<Id>>>);

impl server_rs::GlobalHandler<()> for ServerData<server_rs::ObjectId> {
    fn bind(
        self: Arc<Self>,
        _: &mut server_rs::Handle<()>,
        _: &mut (),
        _: server_rs::ClientId,
        _: server_rs::GlobalId,
        object_id: server_rs::ObjectId,
    ) -> Arc<dyn server_rs::ObjectData<()>> {
        *(self.0.lock().unwrap()) = Some(object_id);
        Arc::new(DoNothingData)
    }
}

impl server_sys::GlobalHandler<()> for ServerData<server_sys::ObjectId> {
    fn bind(
        self: Arc<Self>,
        _: &mut server_sys::Handle<()>,
        _: &mut (),
        _: server_sys::ClientId,
        _: server_sys::GlobalId,
        object_id: server_sys::ObjectId,
    ) -> Arc<dyn server_sys::ObjectData<()>> {
        *(self.0.lock().unwrap()) = Some(object_id);
        Arc::new(DoNothingData)
    }
}

expand_test!(protocol_error, {
    let (tx, rx) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut server = server_backend::Backend::new().unwrap();
    let _client_id = server.insert_client(rx, Arc::new(DoNothingData)).unwrap();
    let mut client = client_backend::Backend::connect(tx).unwrap();

    let object_id = Arc::new(Mutex::new(None));

    // Prepare a global
    server.handle().create_global(
        &interfaces::TEST_GLOBAL_INTERFACE,
        3,
        Arc::new(ServerData(object_id.clone())),
    );

    // get the registry client-side
    let client_display = client.handle().display_id();
    let placeholder = client.handle().placeholder_id(Some((&interfaces::WL_REGISTRY_INTERFACE, 1)));
    let registry_id = client
        .handle()
        .send_request(
            message!(client_display, 1, [Argument::NewId(placeholder)],),
            Some(Arc::new(DoNothingData)),
        )
        .unwrap();
    // create the test global
    let placeholder = client.handle().placeholder_id(Some((&interfaces::TEST_GLOBAL_INTERFACE, 3)));
    client
        .handle()
        .send_request(
            message!(
                registry_id,
                0,
                [
                    Argument::Uint(1),
                    Argument::Str(Box::new(
                        CString::new(interfaces::TEST_GLOBAL_INTERFACE.name.as_bytes()).unwrap(),
                    )),
                    Argument::Uint(3),
                    Argument::NewId(placeholder),
                ],
            ),
            Some(Arc::new(DoNothingData)),
        )
        .unwrap();

    client.flush().unwrap();
    server.dispatch_all_clients(&mut ()).unwrap();

    // get the object_id for the global
    let oid = object_id.lock().unwrap().clone().unwrap();

    // post the error
    server.handle().post_error(oid, 42, CString::new("I don't like you.".as_bytes()).unwrap());

    server.flush(None).unwrap();
    let ret = client.dispatch_events();

    match ret {
        Err(client_backend::WaylandError::Protocol(err)) => {
            assert_eq!(err.code, 42);
            assert_eq!(err.object_id, 3);
            assert_eq!(err.object_interface, "test_global");
            if std::any::TypeId::of::<client_backend::Backend>()
                == std::any::TypeId::of::<client_rs::Backend>()
            {
                // only the RS client backed can retrieve the error message
                assert_eq!(err.message, "I don't like you.");
            }
        }
        _ => panic!("Bad ret: {:?}", ret),
    }
});

expand_test!(client_wrong_id, {
    let (tx, rx) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut server = server_backend::Backend::<()>::new().unwrap();
    let _client_id = server.insert_client(rx, Arc::new(DoNothingData)).unwrap();

    let mut socket = BufferedSocket::new(unsafe { Socket::from_raw_fd(tx.into_raw_fd()) });

    socket
        .write_message(&Message {
            sender_id: 1, // wl_display
            opcode: 1,    // wl_registry
            args: smallvec::smallvec![
                Argument::NewId(3), // should be 2
            ],
        })
        .unwrap();
    socket.flush().unwrap();

    server.dispatch_all_clients(&mut ()).unwrap();
    server.flush(None).unwrap();

    // server should have killed us due to the error, but it might send us that error first
    let ret = socket.fill_incoming_buffers().and_then(|_| socket.fill_incoming_buffers());
    assert!(ret.is_err());
});

expand_test!(client_wrong_opcode, {
    let (tx, rx) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut server = server_backend::Backend::<()>::new().unwrap();
    let _client_id = server.insert_client(rx, Arc::new(DoNothingData)).unwrap();

    let mut socket = BufferedSocket::new(unsafe { Socket::from_raw_fd(tx.into_raw_fd()) });

    socket
        .write_message(&Message {
            sender_id: 1, // wl_display
            opcode: 42,   // inexistant
            args: smallvec::smallvec![],
        })
        .unwrap();
    socket.flush().unwrap();

    server.dispatch_all_clients(&mut ()).unwrap();
    server.flush(None).unwrap();

    // server should have killed us due to the error, but it might send us that error first
    let ret = socket.fill_incoming_buffers().and_then(|_| socket.fill_incoming_buffers());
    assert!(ret.is_err());
});

expand_test!(client_wrong_sender, {
    let (tx, rx) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut server = server_backend::Backend::<()>::new().unwrap();
    let _client_id = server.insert_client(rx, Arc::new(DoNothingData)).unwrap();

    let mut socket = BufferedSocket::new(unsafe { Socket::from_raw_fd(tx.into_raw_fd()) });

    socket
        .write_message(&Message {
            sender_id: 2, // inexistant
            opcode: 0,    //
            args: smallvec::smallvec![],
        })
        .unwrap();
    socket.flush().unwrap();

    server.dispatch_all_clients(&mut ()).unwrap();
    server.flush(None).unwrap();

    // server should have killed us due to the error, but it might send us that error first
    let ret = socket.fill_incoming_buffers().and_then(|_| socket.fill_incoming_buffers());
    assert!(ret.is_err());
});
