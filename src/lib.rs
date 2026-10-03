// SPDX-License-Identifier: GPL-3.0-only

//! Library target for the workspace.
//!
//! The terminal binary keeps its implementation bin-internal; this lib
//! exists so the `cosmic-ext-dropterm-applet` member consumes the V1
//! D-Bus contract from the same code that serves it — one contract,
//! one implementation.

pub mod dbus_state;
