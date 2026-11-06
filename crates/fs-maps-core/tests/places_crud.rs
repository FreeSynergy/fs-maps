//! Places: update, delete, list and get through the `Places` trait over a real `TursoStore`.

mod common;

use common::{ME, names, point, polygon, store};
use fs_maps_core::{Error, Paging, PlaceRepository, Places};
use fs_store::ObjectId;
use serde_json::json;

#[tokio::test]
async fn create_gives_context_iri_and_attribution() {
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME).with_instance("maps.example.org");
    let p: &dyn Places = &repo;
    let o = p.create(&point("Berlin", 13.405, 52.52)).await.unwrap();
    assert_eq!(o.envelope.context.as_deref(), Some("places"));
    let iri = format!("https://maps.example.org/places/{}", o.envelope.id);
    assert_eq!(o.envelope.iri.as_deref(), Some(iri.as_str()));
    assert_eq!(o.envelope.attributed_to.as_deref(), Some(ME));
    assert_eq!(p.get(o.envelope.id).await.unwrap().unwrap(), o);
    assert!(p.get(ObjectId::new()).await.unwrap().is_none());
}

#[tokio::test]
async fn update_replaces_content_and_keeps_the_envelope() {
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME);
    let old = repo.create(&point("Berlin", 13.4, 52.5)).await.unwrap();
    let new = repo
        .update(
            old.envelope.id,
            &polygon(
                "Greater Berlin",
                &[[13.0, 52.0], [14.0, 52.0], [14.0, 53.0], [13.0, 52.0]],
            ),
        )
        .await
        .unwrap();
    assert_eq!(new.envelope.id, old.envelope.id);
    assert_eq!(new.envelope.published, old.envelope.published);
    assert_eq!(new.envelope.iri, old.envelope.iri);
    assert_eq!(new.envelope.name, "Greater Berlin");
    assert_eq!(new.index["geometry_type"], ["Polygon"]);
    assert!(!new.index.contains_key("geo"), "the point index is gone");
    // The old point no longer answers a search on its old name; the new name does.
    assert!(
        repo.find_by_name("Berlin", Paging::default())
            .await
            .unwrap()
            .len()
            == 1
    );
    // Bad input and unknown ids change nothing.
    assert!(
        repo.update(old.envelope.id, &json!({"type":"Feature"}))
            .await
            .is_err()
    );
    assert!(matches!(
        repo.update(ObjectId::new(), &point("x", 0.0, 0.0)).await,
        Err(Error::NotFound(_))
    ));
    assert_eq!(
        repo.get(old.envelope.id)
            .await
            .unwrap()
            .unwrap()
            .envelope
            .name,
        "Greater Berlin"
    );
}

#[tokio::test]
async fn delete_removes_a_place_and_is_undoable() {
    use fs_store::Store;
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME);
    let a = repo.create(&point("A", 1.0, 1.0)).await.unwrap();
    repo.create(&point("B", 2.0, 2.0)).await.unwrap();
    repo.delete(a.envelope.id).await.unwrap();
    assert!(repo.get(a.envelope.id).await.unwrap().is_none());
    assert!(matches!(
        repo.delete(a.envelope.id).await,
        Err(Error::NotFound(_))
    ));
    assert_eq!(names(&repo.list(Paging::default()).await.unwrap()), ["B"]);
    s.undo(ME, 1).await.unwrap();
    assert_eq!(
        names(&repo.list(Paging::default()).await.unwrap()),
        ["A", "B"]
    );
}

#[tokio::test]
async fn list_is_in_name_order_and_paged_by_the_store() {
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME);
    for n in ["delta", "Alpha", "charlie", "Bravo", "echo"] {
        repo.create(&point(n, 1.0, 1.0)).await.unwrap();
    }
    let all = repo.list(Paging::default()).await.unwrap();
    assert_eq!(names(&all), ["Alpha", "Bravo", "charlie", "delta", "echo"]);
    let page = repo.list(Paging::new(2, 1)).await.unwrap();
    assert_eq!(names(&page), ["Bravo", "charlie"]);
    assert!(repo.list(Paging::new(2, 9)).await.unwrap().is_empty());
}

#[tokio::test]
async fn save_and_load_still_work() {
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME);
    let id = repo.save(&point("Berlin", 13.405, 52.52)).await.unwrap();
    assert_eq!(
        repo.load(id).await.unwrap().unwrap(),
        point("Berlin", 13.405, 52.52)
    );
}
