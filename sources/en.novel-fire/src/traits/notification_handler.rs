use buny::{NotificationHandler, alloc::String, prelude::*};

use crate::NovelFire;

impl NotificationHandler for NovelFire {
	fn handle_notification(&self, key: String) {
		println!("Notification: {key}");
	}
}
