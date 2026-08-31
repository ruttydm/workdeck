use dioxus::prelude::*;
use workdeck_api::FixtureWorkdeckClient;
use workdeck_ui::{Area, ComponentGallery, WorkdeckApp, WorkdeckAppProps};

fn main() {
    configure_document();
    let parameters = location_parameters();
    if parameters
        .iter()
        .any(|(key, value)| key == "gallery" && value == "1")
    {
        dioxus_web::launch::launch_virtual_dom(
            VirtualDom::new(ComponentGallery),
            dioxus_web::Config::default(),
        );
        return;
    }
    let scenario = parameters
        .iter()
        .find(|(key, _)| key == "fixture")
        .map(|(_, value)| value.as_str())
        .unwrap_or("polished");
    let client = match scenario {
        "empty" => FixtureWorkdeckClient::empty(),
        "offline" => FixtureWorkdeckClient::offline(),
        _ => FixtureWorkdeckClient::polished(),
    }
    .into_client();
    let initial_area = parameters
        .iter()
        .find(|(key, _)| key == "area")
        .and_then(|(_, value)| Area::from_id(value));
    dioxus_web::launch::launch_virtual_dom(
        VirtualDom::new_with_props(
            WorkdeckApp,
            WorkdeckAppProps {
                client,
                initial_area,
                native_command: None,
            },
        ),
        dioxus_web::Config::default(),
    );
}

fn configure_document() {
    if let Some(document_element) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    {
        let _ = document_element.set_attribute("lang", "en");
    }
}

fn location_parameters() -> Vec<(String, String)> {
    let search = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default();
    search
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((key.to_owned(), value.replace('+', " ")))
        })
        .collect()
}
