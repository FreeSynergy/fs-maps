//! `Places.types` over the wire: the catalog of the data types (ADR 0066).

mod common;

use common::with_server;
use fs_maps_core::describe::{read_catalog, registry};

#[tokio::test(flavor = "current_thread")]
async fn the_catalog_answers_every_type_with_every_command() {
    with_server(|maps| async move {
        let catalog = maps
            .places
            .types_request()
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_catalog()
            .unwrap();
        let answer = catalog.describe_request().send().promise.await.unwrap();
        let read = read_catalog(answer.get().unwrap().get_types().unwrap()).unwrap();
        let expected: Vec<_> = registry().iter().cloned().collect();
        assert_eq!(read, expected);
        assert!(
            read.iter()
                .any(|t| t.name == "Place" && t.find_command("within").is_some())
        );
    })
    .await;
}
