use anyhow::Result;
use compression::compression::IntSeqDecoder;
use quantization::quantization::Quantizer;
use utils::DistanceCalculator;

use crate::ivf::index::Ivf;
use crate::posting_list::combined_file::FixedIndexFile;
use crate::vector::fixed_file::FixedFileVectorStorage;

pub struct IvfReader {
    base_directory: String,
}

impl IvfReader {
    pub fn new(base_directory: String) -> Self {
        Self { base_directory }
    }

    pub fn read<Q: Quantizer, DC: DistanceCalculator, D: IntSeqDecoder<Item = u64>>(
        &self,
    ) -> Result<Ivf<Q, DC, D>> {
        let index_storage = FixedIndexFile::new(format!("{}/index", self.base_directory))?;

        let vector_storage_path = format!("{}/vectors", self.base_directory);
        let vector_storage = FixedFileVectorStorage::<Q::QuantizedT>::new(
            vector_storage_path,
            index_storage.header().quantized_dimension as usize,
        )?;

        let num_clusters = index_storage.header().num_clusters as usize;

        // Read quantizer
        let quantizer_directory = format!("{}/quantizer", self.base_directory);
        let quantizer = Q::read(quantizer_directory).unwrap();

        Ok(Ivf::<_, DC, D>::new(
            vector_storage,
            index_storage,
            num_clusters,
            quantizer,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use compression::elias_fano::ef::{EliasFano, EliasFanoDecoder};
    use compression::noc::noc::{PlainDecoder, PlainEncoder};
    use quantization::noq::noq::{NoQuantizer, NoQuantizerWriter};
    use tempdir::TempDir;
    use utils::distance::l2::L2DistanceCalculator;
    use utils::mem::transmute_u8_to_slice;
    use utils::test_utils::generate_random_vector;

    use super::*;
    use crate::index::Searchable;
    use crate::ivf::builder::{IvfBuilder, IvfBuilderConfig};
    use crate::ivf::writer::IvfWriter;
    use crate::posting_list::combined_file::Version;
    use crate::utils::SearchContext;

    #[test]
    fn test_ivf_reader_elias_fano() {
        let temp_dir = TempDir::new("test_ivf_reader_elias_fano")
            .expect("Failed to create temporary directory");
        let base_directory = temp_dir
            .path()
            .to_str()
            .expect("Failed to convert temporary directory path to string")
            .to_string();
        let num_clusters = 10;
        let num_vectors = 1000;
        let num_features = 4;
        let file_size = 4096;
        let quantizer = NoQuantizer::new(num_features);
        let quantizer_directory = format!("{}/quantizer", base_directory);
        std::fs::create_dir_all(&quantizer_directory)
            .expect("Failed to create quantizer directory");
        let noq_writer = NoQuantizerWriter::new(quantizer_directory);
        assert!(noq_writer.write(&quantizer).is_ok());
        let writer =
            IvfWriter::<_, EliasFano, L2DistanceCalculator>::new(base_directory.clone(), quantizer);

        let mut builder: IvfBuilder<L2DistanceCalculator> = IvfBuilder::new(IvfBuilderConfig {
            max_iteration: 1000,
            batch_size: 4,
            num_clusters,
            num_data_points_for_clustering: num_vectors,
            max_clusters_per_vector: 1,
            distance_threshold: 0.1,
            base_directory: base_directory.clone(),
            memory_size: 1024,
            file_size,
            num_features,
            tolerance: 0.0,
            max_posting_list_size: usize::MAX,
        })
        .expect("Failed to create builder");
        // Generate 1000 vectors of f32, dimension 4
        for i in 0..num_vectors {
            builder
                .add_vector((i + 100) as u64, &generate_random_vector(num_features))
                .expect("Vector should be added");
        }

        assert!(builder.build().is_ok());

        assert!(writer.write(&mut builder, false).is_ok());

        let reader = IvfReader::new(base_directory.clone());
        let index = reader
            .read::<NoQuantizer, L2DistanceCalculator, EliasFanoDecoder>()
            .expect("Failed to read index file");

        // Check if files were created
        assert!(fs::metadata(format!("{}/vectors", base_directory)).is_ok());
        assert!(fs::metadata(format!("{}/index", base_directory)).is_ok());

        // Verify vectors file content
        let mut context = SearchContext::new(true);
        for i in 0..num_vectors {
            let ref_vector = builder
                .vectors()
                .borrow()
                .get(i as u32)
                .expect("Failed to read vector from FileBackedAppendableVectorStorage")
                .to_vec();
            let read_vector = index
                .vector_storage
                .get(i, &mut context)
                .expect("Failed to read vector from FixedFileVectorStorage");
            assert_eq!(ref_vector.len(), read_vector.len());
            for (val_ref, val_read) in ref_vector.iter().zip(read_vector.iter()) {
                assert!((*val_ref - *val_read).abs() < f32::EPSILON);
            }
        }

        // Verify index file content
        // Verify header
        assert_eq!(index.index_storage.header().version, Version::V0);
        assert_eq!(
            index.index_storage.header().num_features,
            num_features as u32
        );
        assert_eq!(
            index.index_storage.header().num_clusters,
            num_clusters as u32
        );
        assert_eq!(index.index_storage.header().num_vectors, num_vectors as u64);
        assert_eq!(
            index.index_storage.header().centroids_len,
            (num_clusters * num_features * size_of::<f32>() + size_of::<u64>()) as u64
        );
        // Verify doc_id_mapping content
        for i in 0..num_vectors {
            let ref_id = builder.doc_id_mapping()[i];
            let read_id = index
                .index_storage
                .get_doc_id(i)
                .expect("Failed to read doc_id from FixedFileVectorStorage");
            assert_eq!(ref_id, read_id);
        }
        // Verify centroid content
        for i in 0..num_clusters {
            let ref_vector = builder
                .centroids()
                .borrow()
                .get(i as u32)
                .expect("Failed to read centroid from FileBackedAppendableVectorStorage")
                .to_vec();
            let read_vector = index
                .index_storage
                .get_centroid(i)
                .expect("Failed to read centroid from FixedFileVectorStorage");
            assert_eq!(ref_vector.len(), read_vector.len());
            for (val_ref, val_read) in ref_vector.iter().zip(read_vector.iter()) {
                assert!((*val_ref - *val_read).abs() < f32::EPSILON);
            }
        }
        // Verify posting list content
        for i in 0..num_clusters {
            let ref_vector = builder
                .posting_lists_mut()
                .get(i as u32)
                .expect("Failed to read vector from FileBackedAppendablePostingListStorage");
            let byte_slice = index
                .index_storage
                .get_posting_list(i)
                .expect("Failed to read vector from FixedIndexFile");
            let decoder = EliasFanoDecoder::new_decoder(byte_slice)
                .expect("Failed to create posting list decoder");
            for (val_ref, val_read) in ref_vector.iter().zip(decoder.get_iterator(byte_slice)) {
                assert_eq!(val_ref, val_read);
            }
        }
    }

    #[test]
    fn test_ivf_reader_read_elias_fano_encoding() {
        // Create reference index (using PlainEncoder/Decoder)
        let temp_dir_ref = TempDir::new("test_ivf_reader_read_elias_fano_encoding_ref")
            .expect("Failed to create ref temporary directory");
        let base_directory_ref = temp_dir_ref
            .path()
            .to_str()
            .expect("Failed to convert ref temporary directory path to string")
            .to_string();
        // Create index using EliasFano
        let temp_dir = TempDir::new("test_ivf_reader_read_elias_fano_encoding")
            .expect("Failed to create temporary directory");
        let base_directory = temp_dir
            .path()
            .to_str()
            .expect("Failed to convert temporary directory path to string")
            .to_string();

        let num_clusters = 10;
        let num_vectors = 1000;
        let num_features = 4;
        let file_size = 4096;

        let quantizer = NoQuantizer::new(num_features);
        let quantizer_directory_ref = format!("{}/quantizer", base_directory_ref);
        std::fs::create_dir_all(&quantizer_directory_ref)
            .expect("Failed to create quantizer directory");
        let noq_writer_ref = NoQuantizerWriter::new(quantizer_directory_ref);
        assert!(noq_writer_ref.write(&quantizer).is_ok());
        let writer_ref = IvfWriter::<_, PlainEncoder, L2DistanceCalculator>::new(
            base_directory_ref.clone(),
            quantizer,
        );
        let quantizer = NoQuantizer::new(num_features);
        let quantizer_directory = format!("{}/quantizer", base_directory);
        std::fs::create_dir_all(&quantizer_directory)
            .expect("Failed to create quantizer directory");
        let noq_writer = NoQuantizerWriter::new(quantizer_directory);
        assert!(noq_writer.write(&quantizer).is_ok());
        let writer =
            IvfWriter::<_, EliasFano, L2DistanceCalculator>::new(base_directory.clone(), quantizer);

        let mut builder: IvfBuilder<L2DistanceCalculator> = IvfBuilder::new(IvfBuilderConfig {
            max_iteration: 1000,
            batch_size: 4,
            num_clusters,
            num_data_points_for_clustering: num_vectors,
            max_clusters_per_vector: 1,
            distance_threshold: 0.1,
            base_directory: base_directory_ref.clone(),
            memory_size: 1024,
            file_size,
            num_features,
            tolerance: 0.0,
            max_posting_list_size: usize::MAX,
        })
        .expect("Failed to create builder");

        // Generate 1000 vectors of f32, dimension 4
        for i in 0..num_vectors {
            let vector = generate_random_vector(num_features);
            builder
                .add_vector((i + 100) as u64, &vector)
                .expect("Vector should be added");
        }

        assert!(builder.build().is_ok());

        assert!(writer_ref.write(&mut builder, false).is_ok());
        assert!(writer.write(&mut builder, false).is_ok());

        let reader_ref = IvfReader::new(base_directory_ref.clone());
        let index_ref = reader_ref
            .read::<NoQuantizer, L2DistanceCalculator, PlainDecoder>()
            .expect("Failed to read ref index file");
        let reader = IvfReader::new(base_directory.clone());
        let index = reader
            .read::<NoQuantizer, L2DistanceCalculator, EliasFanoDecoder>()
            .expect("Failed to read index file");

        let k = 3;
        let num_probes = 2;
        let mut context = SearchContext::new(false);
        // Generate 1000 queries
        for _ in 0..1000 {
            let query = generate_random_vector(num_features);
            let results_ref = index_ref
                .search(&query, k, num_probes, &mut context)
                .expect("IVF search ref should return a result");
            let results = index
                .search(&query, k, num_probes, &mut context)
                .expect("IVF search should return a result");
            assert_eq!(results_ref, results);
        }
    }

    #[test]
    fn test_ivf_reader_read() {
        let temp_dir =
            TempDir::new("test_ivf_reader_read").expect("Failed to create temporary directory");
        let base_directory = temp_dir
            .path()
            .to_str()
            .expect("Failed to convert temporary directory path to string")
            .to_string();
        let num_clusters = 10;
        let num_vectors = 1000;
        let num_features = 4;
        let file_size = 4096;
        let quantizer = NoQuantizer::new(num_features);
        let quantizer_directory = format!("{}/quantizer", base_directory);
        std::fs::create_dir_all(&quantizer_directory)
            .expect("Failed to create quantizer directory");
        let noq_writer = NoQuantizerWriter::new(quantizer_directory);
        assert!(noq_writer.write(&quantizer).is_ok());
        let writer = IvfWriter::<_, PlainEncoder, L2DistanceCalculator>::new(
            base_directory.clone(),
            quantizer,
        );

        let mut builder: IvfBuilder<L2DistanceCalculator> = IvfBuilder::new(IvfBuilderConfig {
            max_iteration: 1000,
            batch_size: 4,
            num_clusters,
            num_data_points_for_clustering: num_vectors,
            max_clusters_per_vector: 1,
            distance_threshold: 0.1,
            base_directory: base_directory.clone(),
            memory_size: 1024,
            file_size,
            num_features,
            tolerance: 0.0,
            max_posting_list_size: usize::MAX,
        })
        .expect("Failed to create builder");
        // Generate 1000 vectors of f32, dimension 4
        for i in 0..num_vectors {
            builder
                .add_vector((i + 100) as u64, &generate_random_vector(num_features))
                .expect("Vector should be added");
        }

        assert!(builder.build().is_ok());

        assert!(writer.write(&mut builder, false).is_ok());

        let reader = IvfReader::new(base_directory.clone());
        let index = reader
            .read::<NoQuantizer, L2DistanceCalculator, PlainDecoder>()
            .expect("Failed to read index file");

        // Check if files were created
        assert!(fs::metadata(format!("{}/vectors", base_directory)).is_ok());
        assert!(fs::metadata(format!("{}/index", base_directory)).is_ok());

        // Verify vectors file content
        let mut context = SearchContext::new(true);
        for i in 0..num_vectors {
            let ref_vector = builder
                .vectors()
                .borrow()
                .get(i as u32)
                .expect("Failed to read vector from FileBackedAppendableVectorStorage")
                .to_vec();
            let read_vector = index
                .vector_storage
                .get(i, &mut context)
                .expect("Failed to read vector from FixedFileVectorStorage");
            assert_eq!(ref_vector.len(), read_vector.len());
            for (val_ref, val_read) in ref_vector.iter().zip(read_vector.iter()) {
                assert!((*val_ref - *val_read).abs() < f32::EPSILON);
            }
        }

        // Verify index file content
        // Verify header
        assert_eq!(index.index_storage.header().version, Version::V0);
        assert_eq!(
            index.index_storage.header().num_features,
            num_features as u32
        );
        assert_eq!(
            index.index_storage.header().num_clusters,
            num_clusters as u32
        );
        assert_eq!(index.index_storage.header().num_vectors, num_vectors as u64);
        assert_eq!(
            index.index_storage.header().centroids_len,
            (num_clusters * num_features * size_of::<f32>() + size_of::<u64>()) as u64
        );
        // Verify doc_id_mapping content
        for i in 0..num_vectors {
            let ref_id = builder.doc_id_mapping()[i];
            let read_id = index
                .index_storage
                .get_doc_id(i)
                .expect("Failed to read doc_id from FixedFileVectorStorage");
            assert_eq!(ref_id, read_id);
        }
        // Verify centroid content
        for i in 0..num_clusters {
            let ref_vector = builder
                .centroids()
                .borrow()
                .get(i as u32)
                .expect("Failed to read centroid from FileBackedAppendableVectorStorage")
                .to_vec();
            let read_vector = index
                .index_storage
                .get_centroid(i)
                .expect("Failed to read centroid from FixedFileVectorStorage");
            assert_eq!(ref_vector.len(), read_vector.len());
            for (val_ref, val_read) in ref_vector.iter().zip(read_vector.iter()) {
                assert!((*val_ref - *val_read).abs() < f32::EPSILON);
            }
        }
        // Verify posting list content
        for i in 0..num_clusters {
            let ref_vector = builder
                .posting_lists_mut()
                .get(i as u32)
                .expect("Failed to read vector from FileBackedAppendablePostingListStorage");
            let read_vector = transmute_u8_to_slice::<u64>(
                index
                    .index_storage
                    .get_posting_list(i)
                    .expect("Failed to read vector from FixedIndexFile"),
            );
            for (val_ref, val_read) in ref_vector.iter().zip(read_vector.iter()) {
                assert_eq!(val_ref, *val_read);
            }
        }
    }

    // Test when the max posting list size is exceeded
    #[test]
    fn test_ivf_reader_read_max_posting_list_size() {
        let temp_dir = TempDir::new("test_ivf_reader_read_max_posting_list_size")
            .expect("Failed to create temporary directory");
        let base_directory = temp_dir
            .path()
            .to_str()
            .expect("Failed to convert temporary directory path to string")
            .to_string();
        let num_clusters = 10;
        let num_vectors = 1000;
        let num_features = 4;
        let file_size = 4096;
        let quantizer = NoQuantizer::new(num_features);
        let quantizer_directory = format!("{}/quantizer", base_directory);
        std::fs::create_dir_all(&quantizer_directory)
            .expect("Failed to create quantizer directory");
        let noq_writer = NoQuantizerWriter::new(quantizer_directory);
        assert!(noq_writer.write(&quantizer).is_ok());

        let writer = IvfWriter::<_, PlainEncoder, L2DistanceCalculator>::new(
            base_directory.clone(),
            quantizer,
        );

        let mut builder: IvfBuilder<L2DistanceCalculator> = IvfBuilder::new(IvfBuilderConfig {
            max_iteration: 1000,
            batch_size: 4,
            num_clusters,
            num_data_points_for_clustering: num_vectors,
            max_clusters_per_vector: 1,
            distance_threshold: 0.1,
            base_directory: base_directory.clone(),
            memory_size: 1024,
            file_size,
            num_features,
            tolerance: 0.0,
            max_posting_list_size: 10,
        })
        .expect("Failed to create builder");
        // Generate 1000 vectors of f32, dimension 4
        for i in 0..num_vectors {
            builder
                .add_vector(i as u64, &generate_random_vector(num_features))
                .expect("Vector should be added");
        }

        assert!(builder.build().is_ok());
        assert!(writer.write(&mut builder, false).is_ok());

        let reader = IvfReader::new(base_directory.clone());
        let index = reader
            .read::<NoQuantizer, L2DistanceCalculator, PlainDecoder>()
            .expect("Failed to read index file");

        let num_centroids = index.num_clusters;

        for i in 0..num_centroids {
            // Assert that posting lists size is less than or equal to max_posting_list_size
            let posting_list_byte_arr = index.index_storage.get_posting_list(i);
            assert!(posting_list_byte_arr.is_ok());
            let posting_list = transmute_u8_to_slice::<u64>(posting_list_byte_arr.unwrap());

            // It's possible that the posting list size is more than max_posting_list_size,
            // but it should be less than 3x.
            assert!(posting_list.len() <= 30);
        }
    }
}
