//! The window of the maps program: the standard shell (ADR 0065), the component Karte and the generated Admin frontend.
//!
//! Pattern: Facade — one call joins the shell, one screen with the map and one screen per data type of the program's
//! type description (the generated frontend, ADR 0066). Nothing is drawn here; the menus are those of the shell
//! with the entry for the map added to View.
//!
//! The generated `admin_window` takes only generated screens, so the map screen cannot be added to it; this file
//! builds the shell the same way (reported: the frontend should have a place for a screen of a component).

use std::sync::Arc;

use fs_admin_frontend::view::{AboutInfo, AdminSurface};
use fs_admin_frontend::window::{ABOUT, HELP, QUIT, TypeView, VIEW_PREFIX, standard_menus};
use fs_admin_frontend::{Session, SystemClock};
use fs_components_app::{AppError, AppWidget, MenuEntry, Screen, Surface};
use fs_components_fields::{Look, Say};
use fs_components_map::{MapSurface, MapViewLabels};
use fs_components_styles::StyleId;

use crate::backend::PlacesBackend;
use crate::map::{PlaceMarkers, ServerTiles, follow};
use crate::remote::PlacesApi;
use crate::words::{English, TITLE};

/// The id of the map screen.
pub const MAP: &str = "map";
/// The address of the help.
pub const HELP_URL: &str = "https://github.com/FreeSynergy/fs-maps";

/// What the About page says.
#[must_use]
pub fn about() -> AboutInfo {
    AboutInfo {
        name: "FreeSynergy.Maps".into(),
        version: format!("v{}", env!("CARGO_PKG_VERSION")),
        origin: "FreeSynergy catalog".into(),
        licence: "MIT OR Apache-2.0".into(),
        repository: "https://github.com/FreeSynergy/fs-maps".into(),
    }
}

/// The window over `api`, with the map screen first. The map starts showing all places whole in `width` x `height`.
///
/// # Errors
/// [`AppError`] if the shell cannot be made; a message for the person if the server has no type description.
pub fn app(api: &Arc<dyn PlacesApi>, width: f64, height: f64) -> Result<AppWidget, String> {
    let say: Arc<dyn Say> = Arc::new(English);
    let descriptions = api
        .types()
        .map_err(|e| format!("the maps server gave no type description: {e:?}"))?;
    let markers = PlaceMarkers::new(Arc::clone(api));
    let map = MapSurface::new(
        MAP,
        MapViewLabels::default(),
        markers.clone(),
        Some(Arc::new(ServerTiles(Arc::clone(api)))),
        markers.start(width, height),
    );
    let mut views = Vec::new();
    for d in descriptions {
        let backend = Arc::new(PlacesBackend::new(Arc::clone(api), d.name.clone()));
        if d.name == "Place" {
            backend.subscribe(Arc::new(follow(&markers, &map)));
        }
        let (id, label) = (d.name.clone(), d.label.clone());
        let session = Session::new(d, backend, Arc::new(SystemClock::new()));
        let surface = AdminSurface::new(
            session,
            Look::new(StyleId::Admin, false),
            Arc::clone(&say),
            None,
        );
        views.push(TypeView {
            id,
            label,
            surface: Arc::new(surface),
        });
    }
    let mut screens = vec![Screen::new(
        MAP,
        say.say("view.map"),
        Arc::new(map.clone()) as Arc<dyn Surface>,
    )];
    screens.extend(views.iter().map(|v| {
        Screen::new(
            v.id.clone(),
            say.say(&v.label),
            Arc::clone(&v.surface) as Arc<dyn Surface>,
        )
    }));
    let app = AppWidget::new(TITLE, screens).map_err(|e: AppError| e.to_string())?;
    let mut menus = standard_menus(&*say, &views);
    if let Some(view) = menus.get_mut(1) {
        view.entries.insert(
            0,
            MenuEntry::new(format!("{VIEW_PREFIX}{MAP}"), say.say("menu.map")),
        );
    }
    let shell = app.clone();
    let surfaces: Vec<_> = views.into_iter().map(|v| (v.id, v.surface)).collect();
    // A marker chosen on the map opens its record in the list of places.
    let (pick_shell, pick_surfaces) = (app.clone(), surfaces.clone());
    markers.on_pick(move |key| {
        if let Some((_, s)) = pick_surfaces.iter().find(|(id, _)| id == "Place") {
            let opened = s
                .session()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .open_detail(key);
            if opened {
                s.refresh();
                pick_shell.select("Place");
            }
        }
    });
    app.set_menus(menus, menu_handler(shell, surfaces, about()))
        .map_err(|e| e.to_string())?;
    Ok(app)
}

type Surfaces = Vec<(String, Arc<AdminSurface>)>;

/// What the entries of the menus do (the same as `admin_window`'s, plus the entry of the map).
fn menu_handler(
    shell: AppWidget,
    surfaces: Surfaces,
    info: AboutInfo,
) -> impl Fn(&str) + Send + Sync + 'static {
    move |id| {
        let current = || shell.with_shell(|s| s.current().map(|c| c.id.clone()));
        match id {
            QUIT => {
                let here = current();
                let unsaved = surfaces
                    .iter()
                    .filter(|(_, s)| s.has_unsaved())
                    .min_by_key(|(id, _)| Some(id) != here.as_ref());
                match unsaved {
                    Some((id, s)) => {
                        shell.select(id);
                        s.warn_unsaved();
                    }
                    None => std::process::exit(0),
                }
            }
            HELP => fs_components_widgets::open_external(HELP_URL),
            ABOUT => {
                // About is a page of the generated screens; from the map, go to the first of them.
                let target = current()
                    .filter(|c| surfaces.iter().any(|(i, _)| i == c))
                    .or_else(|| surfaces.first().map(|(i, _)| i.clone()));
                if let Some(t) = target {
                    shell.select(&t);
                    if let Some((_, s)) = surfaces.iter().find(|(i, _)| *i == t) {
                        s.set_about(Some(info.clone()));
                    }
                }
            }
            other => {
                if let Some(view) = other.strip_prefix(VIEW_PREFIX) {
                    shell.select(view);
                }
            }
        }
    }
}
