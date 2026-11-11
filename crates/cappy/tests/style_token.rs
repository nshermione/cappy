use cappy::InteractionState;
use cappy::prelude::*;

#[style_token(f32)]
enum Depth {
    Near,
    Far,
}

#[test]
fn custom_token_patch_keeps_unset_slots_and_resolves() {
    let mut theme = Theme::light();
    assert!(!theme.has_tokens::<Depth>());
    theme.patch_tokens(Depth::patch().far(20.0));
    assert!(theme.has_tokens::<Depth>());
    assert_eq!(theme.get(Depth::Near), 0.0);
    assert_eq!(theme.get(Depth::Far), 20.0);

    theme.patch_tokens(Depth::patch().near(4.0));
    assert_eq!(theme.get(Depth::Near), 4.0);
    assert_eq!(theme.get(Depth::Far), 20.0);

    let style = theme.resolve(
        None,
        &[],
        &StyleRefinement::default().radius(Depth::Far),
        InteractionState::default(),
        None,
    );
    assert_eq!(style.visual.corner_radius, cappy::Corners::all(20.0));
}

#[test]
#[should_panic(expected = "is not set on this theme")]
fn reading_an_unset_custom_token_panics() {
    let _ = Theme::light().get(Depth::Near);
}
