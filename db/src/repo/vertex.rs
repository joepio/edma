use crate::{
	interface::KeyValuePair,
	storage::Transaction,
	util::{build_bytemap, build_sized, Component},
	Error, PropertyRepository, SimpleTransaction,
};
use gremlin::{GValue, Labels, Vertex, GID};

impl_repository!(VertexRepository(Vertex));

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct VertexResult {
	v: Vertex,
	initialized: bool,
}

impl VertexResult {
	pub fn new(v: Vertex, initialized: bool) -> Self {
		VertexResult {
			v,
			initialized,
		}
	}

	pub fn v(&self) -> Vertex {
		self.v.clone()
	}

	pub fn initialized(&self) -> bool {
		self.initialized
	}
}

type RepositoryResult<T> = Result<T, Error>;

impl<'a> VertexRepository<'a> {
	/// The V()-step is meant to read vertices from the graph and is usually
	/// used to start a GraphTraversal, but can also be used mid-traversal.
	/// [Documentation](https://tinkerpop.apache.org/docs/current/reference/#v-step)
	pub async fn v(
		&self,
		tx: &Transaction,
		ids: &Vec<GValue>,
	) -> RepositoryResult<Vec<VertexResult>> {
		let cf = self.cf();
		match ids.first() {
			Some(id) => {
				let key = build_sized(Component::GValue(id));
				let vertex = tx.get(cf, key.to_vec()).await.unwrap();
				Ok(vec![match vertex {
					Some(v) => VertexResult {
						v: self.from_pair(&(key, v)).unwrap(),
						initialized: true,
					},
					_ => VertexResult {
						v: self.from_pair(&(key, vec![])).unwrap(),
						initialized: false,
					},
				}])
			}
			_ => self.iterate_all().await,
		}
	}

	pub async fn new_v(
		&self,
		tx: &mut Transaction,
		labels: &Vec<GValue>,
	) -> RepositoryResult<VertexResult> {
		let new_v = &mut Vertex::default();
		self.add_v(tx, new_v, labels, false).await
	}

	/// The addV()-step is used to add vertices to the graph (map/sideEffect).
	/// For every incoming object, a vertex is created. Moreover, GraphTraversalSource maintains an addV() method.
	/// [Documentation](https://tinkerpop.apache.org/docs/current/reference/#addvertex-step)
	pub async fn add_v(
		&self,
		tx: &mut Transaction,
		v: &mut Vertex,
		labels: &Vec<GValue>,
		initialized: bool,
	) -> RepositoryResult<VertexResult> {
		let mut serialized_labels = Vec::<String>::new();
		for label in labels.iter() {
			let val = label.get::<String>().unwrap();
			serialized_labels.push(val.to_string());

			v.add_label(val);
		}
		let labels = Labels::from(serialized_labels);

		// build Label byte (length : usize, data: LabelType)
		let mut bytes = vec![];
		for label in labels.0.iter() {
			let byte = build_sized(Component::Label(label));
			bytes.push(byte);
		}
		let cf = self.cf();
		let key = build_sized(Component::GID(v.id()));
		println!("add v - key: {:?}", key);
		let val = bytes.concat();

		tx.set(cf, key, val).await.unwrap();

		Ok(VertexResult {
			v: v.clone(),
			initialized,
		})
	}

	// If there is no vertices defined, initialized with default option
	pub async fn new_property(
		&self,
		tx: &mut Transaction,
		args: &Vec<GValue>,
	) -> RepositoryResult<VertexResult> {
		let vertex = &mut Vertex::default();
		self.property(vertex, tx, args, false).await
	}

	pub async fn property(
		&self,
		v: &mut Vertex,
		tx: &mut Transaction,
		args: &Vec<GValue>,
		initialized: bool,
	) -> RepositoryResult<VertexResult> {
		let property_repo = PropertyRepository::new(self.ds_ref);
		let (label, value) = (&args[0], &args[1]);
		let property = property_repo.property(tx, v.id(), label, value).await.unwrap();
		v.add_property(property);
		Ok(VertexResult {
			initialized,
			v: v.clone(),
		})
	}

	fn from_pair(&self, p: &KeyValuePair) -> RepositoryResult<Vertex> {
		let (k, v) = p;
		// Handle deserializing and rebuild vertex stream
		let bytemap = &build_bytemap(vec!["gid"], k.to_vec());
		let gid = GID::Bytes(bytemap.get("gid").unwrap().to_vec());
		let mut vertex = Vertex::partial_new(gid);
		// handle deserializing label data of vertex
		let mut i = 0;
		while i < v.len() {
			let len = *v[i..i + 1].first().unwrap();
			let usize_len = len as usize;
			let end = usize_len + i + 1;
			let data = &v[i + 1..end];
			let label = String::from_utf8(data.to_vec()).unwrap();
			vertex.add_label(label);
			i += usize_len + 1;
		}

		Ok(vertex)
	}

	pub async fn drop_v(&self, tx: &mut Transaction, id: GID) -> Result<(), Error> {
		let cf = self.cf();
		let value = id.get::<String>().unwrap();
		let gid = &GID::from(value.to_string());
		let key = build_sized(Component::GID(gid));
		tx.del(cf, key).await.unwrap();
		Ok(())
	}

	async fn iterate(
		&self,
		iterator: Vec<Result<KeyValuePair, Error>>,
	) -> RepositoryResult<Vec<VertexResult>> {
		let mut result: Vec<VertexResult> = vec![];
		for pair in iterator.iter() {
			let p_ref = pair.as_ref().unwrap();
			let vertex = self.from_pair(p_ref).unwrap();
			result.push(VertexResult {
				v: vertex,
				initialized: true,
			});
		}

		Ok(result)
	}

	pub async fn iterate_all(&self) -> RepositoryResult<Vec<VertexResult>> {
		let tx = self.ds().transaction(false).unwrap();
		let cf = self.cf();

		let iterator = tx.iterate(cf).await.unwrap();
		self.iterate(iterator).await
	}
}
