//! Listing and searching places inside the store.
//!
//! Pattern: Builder consumer — each search assembles one `Query` (type, conditions, order, page)
//! that the store runs, so nothing is read into memory beyond the requested page. The area test
//! is a set of index conditions (see `bounds`), the name test a name / index `Contains`.

use fs_objects::{Cond, Op, Order, Query, StoredObject};

use super::adapter::PLACE_TYPE;
use super::bounds::Area;
use super::contract::Paging;
use super::repository::PlaceRepository;
use crate::error::Result;

/// Index key holding every name of a place (`name`, `name:*`, `title`).
pub(super) const NAME_KEY: &str = "name";

fn base(page: Paging) -> Query {
    let mut q = Query::new()
        .of_type(PLACE_TYPE)
        .ordered(Order::NameAsc)
        .offset(page.offset);
    q.limit = page.limit;
    q
}

impl PlaceRepository<'_> {
    /// Places in name order, paged.
    pub async fn list_places(&self, page: Paging) -> Result<Vec<StoredObject>> {
        Ok(self.reads.query(&base(page)).await?)
    }

    /// Places whose geometry's bounding box overlaps `area`.
    pub async fn places_within(&self, area: &Area, page: Paging) -> Result<Vec<StoredObject>> {
        let mut q = base(page);
        let (all, any) = area.conditions();
        q.conds.extend(all);
        q.any.extend(any);
        Ok(self.reads.query(&q).await?)
    }

    /// Places with a name containing `term`, ignoring case.
    pub async fn places_named(&self, term: &str, page: Paging) -> Result<Vec<StoredObject>> {
        let mut q = base(page);
        q.any.push(Cond::Name {
            op: Op::Contains,
            value: term.into(),
        });
        q.any.push(Cond::Index {
            key: NAME_KEY.into(),
            op: Op::Contains,
            value: term.into(),
        });
        Ok(self.reads.query(&q).await?)
    }
}
