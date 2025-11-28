use buny::{
	AlternateCoverProvider, Novel, Result,
	alloc::{String, Vec, vec},
};

use crate::NovelFire;

impl AlternateCoverProvider for NovelFire {
	fn get_alternate_covers(&self, _novel: Novel) -> Result<Vec<String>> {
		Ok(vec!["https://buny.app/images/icon.png".into()])
	}
}
