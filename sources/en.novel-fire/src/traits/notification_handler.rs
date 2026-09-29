use buny::{NotificationHandler, alloc::String};

use crate::NovelFire;

impl NotificationHandler for NovelFire {
	fn handle_notification(&self, _key: String) {}
}
