//! A shared theme. [`ThemeHandle`] is a cloneable handle, the same way [`Context`](crate::Context)
//! is a handle to one runtime.

use std::fmt;
use std::rc::Rc;

use cappy_style::{Theme, Tokens};

use crate::reactive::{Reactive, Signal};

/// A shared theme. Cloning shares it; the stylesheet is not copied.
///
/// Build a [`Theme`], then pass it to [`Context::set_themes`](crate::Context::set_themes).
/// [`Context::use_theme`](crate::Context::use_theme) reads the one that is active.
#[derive(Clone)]
pub struct ThemeHandle {
    theme: Rc<Theme>,
}

impl ThemeHandle {
    pub fn name(&self) -> &str {
        self.theme.name()
    }

    pub fn tokens(&self) -> &Tokens {
        self.theme.tokens()
    }

    pub fn has_class(&self, name: &str) -> bool {
        self.theme.has_class(name)
    }

    pub(crate) fn get(&self) -> &Theme {
        &self.theme
    }

    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.theme, &other.theme)
    }
}

impl From<Theme> for ThemeHandle {
    fn from(theme: Theme) -> Self {
        Self {
            theme: Rc::new(theme),
        }
    }
}

impl fmt::Debug for ThemeHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ThemeHandle")
            .field("name", &self.name())
            .finish()
    }
}

/// The themes registered on a [`Context`](crate::Context), in registration order.
#[derive(Clone)]
pub struct ThemeCatalog {
    themes: Rc<[ThemeHandle]>,
}

impl ThemeCatalog {
    pub(crate) fn from_themes(themes: Vec<Theme>) -> Self {
        assert!(!themes.is_empty(), "set_themes needs at least one theme");
        for (index, theme) in themes.iter().enumerate() {
            if themes[..index]
                .iter()
                .any(|earlier| earlier.name() == theme.name())
            {
                panic!("duplicate theme name \"{}\"", theme.name());
            }
        }
        let themes: Rc<[ThemeHandle]> = themes.into_iter().map(ThemeHandle::from).collect();
        Self { themes }
    }

    pub fn len(&self) -> usize {
        self.themes.len()
    }

    pub fn get(&self, index: usize) -> Option<&ThemeHandle> {
        self.themes.get(index)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ThemeHandle> {
        self.themes.iter()
    }

    /// The theme after `current`, wrapping back to the first.
    ///
    /// A theme that is not in this catalog returns the first theme.
    pub fn cycle(&self, current: &ThemeHandle) -> ThemeHandle {
        match self
            .themes
            .iter()
            .position(|candidate| candidate.ptr_eq(current))
        {
            Some(index) => self.themes[(index + 1) % self.themes.len()].clone(),
            None => self.themes[0].clone(),
        }
    }

    pub(crate) fn by_name(&self, name: &str) -> Option<ThemeHandle> {
        self.themes
            .iter()
            .find(|theme| theme.name() == name)
            .cloned()
    }

    pub(crate) fn first(&self) -> ThemeHandle {
        self.themes[0].clone()
    }
}

/// The themes of one context. Copying copies the signals, not the themes.
///
/// [`Themes::with`] and [`Themes::get`] read the active theme and subscribe the current tracking
/// scope, so an `element!` binding that calls them reruns when it changes. [`Themes::set`] selects
/// a theme. [`Themes::catalog`] is the registered list.
#[derive(Clone, Copy)]
pub struct Themes {
    pub(crate) active: Signal<ThemeHandle>,
    pub(crate) catalog: Signal<ThemeCatalog>,
}

impl Themes {
    /// Selects `theme`. A registered name uses that shared theme.
    pub fn set(&self, theme: impl Into<ThemeHandle>) {
        let theme = theme.into();
        let name = theme.name().to_owned();
        let resolved = self
            .catalog
            .with_untracked(|catalog| catalog.by_name(&name).unwrap_or(theme));
        self.active.set(resolved);
    }

    /// The active theme. Subscribes the current tracking scope.
    pub fn get(&self) -> ThemeHandle {
        self.active.get()
    }

    /// Reads the active theme and subscribes the current tracking scope.
    pub fn with<R>(&self, read: impl FnOnce(&ThemeHandle) -> R) -> R {
        self.active.with(read)
    }

    /// The theme after the active one, wrapping back to the first.
    ///
    /// A theme that is not in the catalog returns the first theme. This read does not subscribe.
    pub fn cycle(&self) -> ThemeHandle {
        let current = self.active.with_untracked(|theme| theme.clone());
        self.catalog
            .with_untracked(|catalog| catalog.cycle(&current))
    }

    /// The active theme signal, for an `element!` binding.
    pub fn active(&self) -> Reactive<ThemeHandle> {
        Reactive::from_signal(self.active)
    }

    /// The registered themes, in order. Reading the signal in `element!` reruns when the catalog
    /// is replaced.
    pub fn catalog(&self) -> Reactive<ThemeCatalog> {
        Reactive::from_signal(self.catalog)
    }
}

impl fmt::Debug for Themes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Themes")
            .field(
                "active",
                &self.active.with_untracked(|theme| theme.name().to_owned()),
            )
            .finish()
    }
}

impl fmt::Debug for ThemeCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ThemeCatalog")
            .field(
                "names",
                &self
                    .themes
                    .iter()
                    .map(ThemeHandle::name)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}
