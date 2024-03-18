use std::sync::Arc;

use cqrs_es::{
    persist::{GenericQuery, ViewRepository},
    Aggregate, View,
};

/// Returns a new `GenericQuery` instance.
pub fn generic_query<R, A, V>(view_repository: Arc<R>) -> GenericQuery<R, V, A>
where
    R: ViewRepository<V, A>,
    A: Aggregate,
    V: View<A>,
{
    let mut generic_query = GenericQuery::new(view_repository);
    generic_query.use_error_handler(Box::new(|e| println!("{}", e)));

    generic_query
}
