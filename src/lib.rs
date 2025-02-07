mod iterator;

use pyo3::create_exception;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyInt, PyString, PyDict};
use rocksdb::{Options, DB, WriteBatch, DBCompressionType};
use std::str;
use std::sync::Arc;
use std::time::Duration;

create_exception!(rocksdb3, RocksDBError, PyRuntimeError);

/// Python bindings for rocksdb.
#[pymodule]
fn rocksdb3(_py: Python, m: &PyModule) -> PyResult<()> {
    /// A RocksDB database.
    #[pyclass]
    struct RocksDB {
        db: Arc<DB>,
        path: Vec<u8>,
    }

    /// Batch writer.
    #[pyclass]
    pub struct WriterBatch {
        writer: Option<WriteBatch>,
    }

    #[pymethods]
    impl RocksDB {
        /// The path of the database.
        #[getter(path)]
        fn get_path<'py>(&self, py: Python<'py>) -> &'py PyString {
            return PyString::new(py, str::from_utf8(&self.path).unwrap());
        }

        /// Return the bytes associated with a key value.
        ///
        /// Positional arguments:
        /// - `key` (required): Name to get.
        fn get<'py>(
            &mut self,
            py: Python<'py>,
            key: &PyBytes,
        ) -> PyResult<Option<&'py PyBytes>> {
            match self.db.get(key.as_bytes()) {
                Ok(Some(value)) => Ok(Some(PyBytes::new(py, &value))),
                Ok(None) => return Ok(None),
                Err(e) => {
                    return Err(RocksDBError::new_err(format!(
                        "can not get key {}: {}",
                        key, e,
                    )))
                }
            }
        }

        /// Set the database entry for "key" to "value".
        /// If "key" already exists, it will be overwritten.
        ///
        /// Positional arguments:
        /// - `key` (required): Name for this entry.
        /// - `value` (required): Data for this entry.
        fn put(&mut self, key: &PyBytes, value: &PyBytes) -> PyResult<()> {
            match self.db.put(key.as_bytes(), value.as_bytes()) {
                Ok(_) => Ok(()),
                Err(e) => {
                    return Err(RocksDBError::new_err(format!(
                        "can not put key {}: {}",
                        key, e,
                    )))
                }
            }
        }

        /// Remove the database entry for "key".
        ///
        /// Positional arguments:
        /// - `key` (required): Name to delete.
        fn delete(&mut self, key: &PyBytes) -> PyResult<()> {
            match self.db.delete(key.as_bytes()) {
                Ok(_) => Ok(()),
                Err(e) => {
                    return Err(RocksDBError::new_err(format!(
                        "can not delete key {}: {}",
                        key, e,
                    )))
                }
            }
        }

        /// Write apply the given `batch` to the database.
        ///
        /// Positional arguments:
        /// - `batch` (required): WriteBatch instance.
        fn write(&self, batch: &mut WriterBatch) -> PyResult<()> {
            let wr = batch.get().unwrap();
            let len = wr.len();

            match self.db.write(wr) {
                Ok(_) => Ok(()),
                Err(e) => {
                    return Err(RocksDBError::new_err(format!(
                        "can not write batch of {} elements: {}",
                        len, e,
                    )))
                }
            }
        }

        /// Opens an iterator using default options.
        fn get_iter(&mut self) -> PyResult<iterator::RocksDBIterator> {
            Ok(iterator::RocksDBIterator::new(self.db.clone()))
        }

        /// Tries to catch up with the primary by reading as much as possible from the log files.
        fn try_catch_up_with_primary(&mut self) -> PyResult<()> {
            match self.db.try_catch_up_with_primary() {
                Ok(_) => Ok(()),
                Err(e) => {
                    return Err(RocksDBError::new_err(format!(
                        "can not catch up with the primary: {}", e,
                    )))
                }
            }
        }
    }

    #[pymethods]
    impl WriterBatch {
        #[new]
        fn new() -> PyResult<Self> {
            Ok(WriterBatch{writer: Some(WriteBatch::default())})
        }

        /// Append new "key" and "value" in batch.
        ///
        /// Positional arguments:
        /// - `key` (required): Name for this entry.
        /// - `value` (required): Data for this entry.
        fn put(&mut self, key: &PyBytes, value: &PyBytes) -> PyResult<()> {
            match &mut self.writer {
                Some(inner) => Ok(inner.put(key.as_bytes(), value.as_bytes())),
                None => Err(RocksDBError::new_err(
                    "batch writer is invalid. new writer is required",
                )),
            }
        }

        /// Remove "key" from batch.
        ///
        /// Positional arguments:
        /// - `key` (required): Name to delete.
        fn delete(&mut self, key: &PyBytes) -> PyResult<()> {
            match &mut self.writer {
                Some(inner) => Ok(inner.delete(key.as_bytes())),
                None => Err(RocksDBError::new_err(
                    "batch writer is invalid. new writer is required",
                )),
            }
        }

        /// Clear the batch.
        fn clear(&mut self) -> PyResult<()> {
            match &mut self.writer {
                Some(inner) => Ok(inner.clear()),
                None => Err(RocksDBError::new_err(
                    "batch writer is invalid. new writer is required",
                )),
            }
        }
    }

    impl WriterBatch {
        pub fn get(&mut self) -> PyResult<WriteBatch> {
            if let Some(inner) = self.writer.take() {
                Ok(inner)
            } else {
                Err(RocksDBError::new_err(
                    "batch writer is invalid. new writer is required",
                ))
            }
        }
    }

    /// Opens a database with default options.
    ///
    /// Positional arguments:
    /// - `path` (required): Path of the database to open.
    #[pyfn(m, "open_default")]
    fn open_default(path: &str) -> PyResult<RocksDB> {
        match DB::open_default(path) {
            Ok(db) => Ok(RocksDB {
                db: Arc::new(db),
                path: path.as_bytes().to_vec(),
            }),
            Err(e) => {
                return Err(RocksDBError::new_err(format!(
                    "can not open {}: {}",
                    path, e,
                )))
            }
        }
    }

    /// Opens a database with options.
    ///
    /// Positional arguments:
    /// - `path` (required): Path of the database to open.
    #[pyfn(m, "open", kwds="**")]
    fn open(path: &str, kwds: Option<&PyDict>) -> PyResult<RocksDB> {
        let mut opts = Options::default();
        if let Some(kwds) = kwds {
            if let Some(compression_type) = kwds.get_item("compression_type") {
                if let Ok(compression_type) = compression_type.extract::<String>() {
                    match compression_type.as_str() {
                        "zstd" => opts.set_compression_type(DBCompressionType::Zstd),
                        "lz4" => opts.set_compression_type(DBCompressionType::Lz4),
                        "snappy" => opts.set_compression_type(DBCompressionType::Snappy),
                        _ => {
                            return Err(RocksDBError::new_err(format!(
                                "unknown compression_type: {}",
                                compression_type,
                            )))
                        }
                    }
                }
            }
        }
        match DB::open(&opts, path) {
            Ok(db) => Ok(RocksDB {
                db: Arc::new(db),
                path: path.as_bytes().to_vec(),
            }),
            Err(e) => {
                return Err(RocksDBError::new_err(format!(
                    "can not open {}: {}",
                    path, e,
                )))
            }
        }
    }

    /// Opens the database with TTL compaction filter.
    ///
    /// Positional arguments:
    /// - `path` (required): Path of the database to open.
    /// - `duration` (required): Duration of the TTL.
    #[pyfn(m, "open_with_ttl")]
    fn open_with_ttl(path: &str, ttl: &PyInt) -> PyResult<RocksDB> {
        let secs = ttl.extract::<u64>().unwrap();
        let duration = Duration::from_secs(secs);

        let mut opts = Options::default();
        opts.create_if_missing(true);

        match DB::open_with_ttl(&opts, path, duration) {
            Ok(db) => Ok(RocksDB {
                db: Arc::new(db),
                path: path.as_bytes().to_vec(),
            }),
            Err(e) => {
                return Err(RocksDBError::new_err(format!(
                    "can not open with {} with ttl {} seconds: {}",
                    path, duration.as_secs(), e,
                )))
            }
        }
    }

    /// Opens the database as a read-only secondary instance.
    ///
    /// Positional arguments:
    /// - `primary_path` (required): Path of the primary database instance.
    /// - `secondary_path` (required): Path of the secondary database to open.
    #[pyfn(m, "open_as_secondary")]
    fn open_as_secondary(primary_path: &str, secondary_path: &str) -> PyResult<RocksDB> {
        match DB::open_as_secondary(&Options::default(), primary_path, secondary_path) {
            Ok(db) => Ok(RocksDB {
                db: Arc::new(db),
                path: secondary_path.as_bytes().to_vec(),
            }),
            Err(e) => {
                return Err(RocksDBError::new_err(format!(
                    "can not open secondary instance {} with {}: {}",
                    secondary_path, primary_path, e,
                )))
            }
        }
    }

    /// Repair the database cannot be opened.
    ///
    /// If a DB cannot be opened, you may attempt to call this method to
    /// resurrect as much of the contents of the database as possible.
    /// Some data may be lost, so be careful when calling this function
    /// on a database that contains important information.
    ///
    /// Positional arguments:
    /// - `path` (required): Path of the database to repair.
    #[pyfn(m, "repair")]
    fn repair(path: &str) -> PyResult<()> {
        match DB::repair(&Options::default(), path) {
            Ok(()) => Ok(()),
            Err(e) => {
                return Err(RocksDBError::new_err(format!(
                    "can not repair {}: {}",
                    path, e,
                )))
            }
        }
    }

    /// Destroy the contents of the specified database.
    /// **Be very careful using this method.**
    ///
    /// Positional arguments:
    /// - `path` (required): Path of the database to destroy.
    #[pyfn(m, "destroy")]
    fn destroy(path: &str) -> PyResult<()> {
        match DB::destroy(&Options::default(), path) {
            Ok(()) => Ok(()),
            Err(e) => {
                return Err(RocksDBError::new_err(format!(
                    "can not destroy {}: {}",
                    path, e,
                )))
            }
        }
    }

    m.add_class::<RocksDB>()?;
    m.add_class::<WriterBatch>()?;
    m.add("RocksDBError", _py.get_type::<RocksDBError>())?;
    Ok(())
}
