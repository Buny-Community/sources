use buny::{
	DynamicSettings, Result, Setting, ToggleSetting,
	alloc::{Vec, vec},
	imports::defaults::defaults_get,
};

use crate::NovelFire;

// if you need to serve settings dynamically, use the DynamicSettings trait
// again, this shouldn't be used for static settings
impl DynamicSettings for NovelFire {
	fn get_dynamic_settings(&self) -> Result<Vec<Setting>> {
		let toggle_value = defaults_get::<bool>("setting");
		let mut settings = vec![
			ToggleSetting {
				key: "setting".into(),
				title: "Toggle".into(),
				notification: Some("test".into()),
				refreshes: Some(vec!["settings".into()]),
				..Default::default()
			}
			.into(),
		];
		if let Some(value) = toggle_value {
			if value {
				settings.push(
					ToggleSetting {
						key: "setting2".into(),
						title: "Toggle 2".into(),
						..Default::default()
					}
					.into(),
				);
			}
		}
		Ok(settings)
	}
}
