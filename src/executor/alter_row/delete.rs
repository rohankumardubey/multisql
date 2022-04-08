use {
	crate::{
		data::{Schema},
		executor::types::ColumnInfo,
		Column, ExecuteError, Glue, MetaRecipe, Payload, PlannedRecipe, Result, Value, ComplexTableName
	},
	sqlparser::ast::{Expr, ObjectName},
};

impl Glue {
	pub async fn delete(
		&mut self,
		table_name: &ObjectName,
		selection: &Option<Expr>,
	) -> Result<Payload> {
		let ComplexTableName{name: table_name, database, ..} = table_name.try_into()?;
		let Schema {
			column_defs,
			indexes,
			..
		} = self
			.get_database(&database)?
			.fetch_schema(&table_name)
			.await?
			.ok_or(ExecuteError::TableNotExists)?;

		let columns = column_defs
			.clone()
			.into_iter()
			.map(|Column { name, .. }| ColumnInfo::of_name(name))
			.collect::<Vec<ColumnInfo>>();
		let filter = selection
			.clone()
			.map(|selection| {
				PlannedRecipe::new(
					MetaRecipe::new(selection)?.simplify_by_context(&*self.get_context()?)?,
					&columns,
				)
			})
			.unwrap_or(Ok(PlannedRecipe::TRUE))?;

		let keys = self
			.get_database(&database)?
			.scan_data(&table_name)
			.await?
			.filter_map(|row_result| {
				let (key, row) = match row_result {
					Ok(keyed_row) => keyed_row,
					Err(error) => return Some(Err(error)),
				};

				let row = row.0;

				let confirm_constraint = filter.confirm_constraint(&row);
				match confirm_constraint {
					Ok(true) => Some(Ok(key)),
					Ok(false) => None,
					Err(error) => Some(Err(error)),
				}
			})
			.collect::<Result<Vec<Value>>>()?;

		let num_keys = keys.len();

		let database = &mut **self.get_mut_database(&None)?;
		let result = database
			.delete_data(&table_name, keys)
			.await
			.map(|_| Payload::Delete(num_keys))?;

		for index in indexes.iter() {
			index.reset(database, &table_name, &column_defs).await?; // TODO: Not this; optimise
		}
		Ok(result)
	}
}
