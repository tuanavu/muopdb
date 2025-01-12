use anyhow::Result;
use compression::noc::noc::PlainDecoder;
use quantization::noq::noq::NoQuantizer;
use quantization::quantization::Quantizer;
use utils::distance::l2::L2DistanceCalculator;

use super::index::Spann;
use crate::hnsw::reader::HnswReader;
use crate::ivf::reader::IvfReader;

pub struct SpannReader {
    base_directory: String,
    centroids_index_offset: usize,
    centroids_vector_offset: usize,
    ivf_index_offset: usize,
    ivf_vector_offset: usize,
}

impl SpannReader {
    pub fn new(base_directory: String) -> Self {
        Self {
            base_directory,
            centroids_index_offset: 0,
            centroids_vector_offset: 0,
            ivf_index_offset: 0,
            ivf_vector_offset: 0,
        }
    }

    pub fn new_with_offsets(
        base_directory: String,
        centroids_index_offset: usize,
        centroids_vector_offset: usize,
        ivf_index_offset: usize,
        ivf_vector_offset: usize,
    ) -> Self {
        Self {
            base_directory,
            centroids_index_offset,
            centroids_vector_offset,
            ivf_index_offset,
            ivf_vector_offset,
        }
    }

    pub fn read<Q: Quantizer>(&self) -> Result<Spann<Q>> {
        let posting_list_path = format!("{}/ivf", self.base_directory);
        let centroid_path = format!("{}/centroids", self.base_directory);

        let centroids = HnswReader::new_with_offset(
            centroid_path,
            self.centroids_index_offset,
            self.centroids_vector_offset,
        )
        .read::<NoQuantizer<L2DistanceCalculator>>()?;
        let posting_lists = IvfReader::new_with_offset(
            posting_list_path,
            self.ivf_index_offset,
            self.ivf_vector_offset,
        )
        .read::<Q, L2DistanceCalculator, PlainDecoder>()?;

        Ok(Spann::<_>::new(centroids, posting_lists))
    }
}

#[cfg(test)]
mod tests {

    use config::enums::{IntSeqEncodingType, QuantizerType};
    use quantization::pq::pq::ProductQuantizer;
    use tempdir::TempDir;
    use utils::mem::transmute_u8_to_slice;
    use utils::test_utils::generate_random_vector;

    use super::*;
    use crate::spann::builder::{SpannBuilder, SpannBuilderConfig};
    use crate::spann::writer::SpannWriter;

    #[test]
    fn test_read() {
        let temp_dir = TempDir::new("test_read").unwrap();
        let base_directory = temp_dir.path().to_str().unwrap().to_string();
        let num_clusters = 10;
        let num_vectors = 1000;
        let num_features = 4;
        let file_size = 4096;
        let balance_factor = 0.0;
        let max_posting_list_size = usize::MAX;
        let mut builder = SpannBuilder::new(SpannBuilderConfig {
            max_neighbors: 10,
            max_layers: 2,
            ef_construction: 100,
            vector_storage_memory_size: 1024,
            vector_storage_file_size: file_size,
            num_features,
            subvector_dimension: 8,
            num_bits: 8,
            num_training_rows: 50,
            quantizer_type: QuantizerType::NoQuantizer,
            max_iteration: 1000,
            batch_size: 4,
            num_clusters,
            num_data_points_for_clustering: num_vectors,
            max_clusters_per_vector: 1,
            distance_threshold: 0.1,
            posting_list_encoding_type: IntSeqEncodingType::PlainEncoding,
            base_directory: base_directory.clone(),
            memory_size: 1024,
            file_size,
            tolerance: balance_factor,
            max_posting_list_size,
            reindex: false,
        })
        .unwrap();

        // Generate 1000 vectors of f32, dimension 4
        for i in 0..num_vectors {
            builder
                .add(i as u64, &generate_random_vector(num_features))
                .unwrap();
        }
        builder.build().unwrap();
        let spann_writer = SpannWriter::new(base_directory.clone());
        spann_writer.write(&mut builder).unwrap();

        let spann_reader = SpannReader::new(base_directory.clone());
        let spann = spann_reader
            .read::<NoQuantizer<L2DistanceCalculator>>()
            .unwrap();

        let centroids = spann.get_centroids();
        let posting_lists = spann.get_posting_lists();
        assert_eq!(
            posting_lists.num_clusters,
            centroids.vector_storage.num_vectors
        );
    }

    #[test]
    fn test_read_pq() {
        let temp_dir = TempDir::new("test_read_pq").unwrap();
        let base_directory = temp_dir.path().to_str().unwrap().to_string();
        let num_clusters = 10;
        let num_vectors = 1000;
        let num_features = 4;
        let file_size = 4096;
        let balance_factor = 0.0;
        let max_posting_list_size = usize::MAX;
        let mut builder = SpannBuilder::new(SpannBuilderConfig {
            max_neighbors: 10,
            max_layers: 2,
            ef_construction: 100,
            vector_storage_memory_size: 1024,
            vector_storage_file_size: file_size,
            num_features,
            subvector_dimension: 2,
            num_bits: 2,
            num_training_rows: 50,
            quantizer_type: QuantizerType::ProductQuantizer,
            max_iteration: 1000,
            batch_size: 4,
            num_clusters,
            num_data_points_for_clustering: num_vectors,
            max_clusters_per_vector: 1,
            distance_threshold: 0.1,
            posting_list_encoding_type: IntSeqEncodingType::PlainEncoding,
            base_directory: base_directory.clone(),
            memory_size: 1024,
            file_size,
            tolerance: balance_factor,
            max_posting_list_size,
            reindex: false,
        })
        .unwrap();

        // Generate 1000 vectors of f32, dimension 4
        for i in 0..num_vectors {
            builder
                .add(i as u64, &generate_random_vector(num_features))
                .unwrap();
        }
        builder.build().unwrap();
        let spann_writer = SpannWriter::new(base_directory.clone());
        spann_writer.write(&mut builder).unwrap();

        let spann_reader = SpannReader::new(base_directory.clone());
        let spann = spann_reader
            .read::<ProductQuantizer<L2DistanceCalculator>>()
            .unwrap();

        let centroids = spann.get_centroids();
        let posting_lists = spann.get_posting_lists();
        assert_eq!(
            posting_lists.num_clusters,
            centroids.vector_storage.num_vectors
        );
        // Verify posting list content
        for i in 0..num_clusters {
            let ref_vector = builder
                .ivf_builder
                .posting_lists_mut()
                .get(i as u32)
                .expect("Failed to read vector for SPANN built from builder");
            let read_vector = transmute_u8_to_slice::<u64>(
                posting_lists
                    .index_storage
                    .get_posting_list(i)
                    .expect("Failed to read vector for SPANN read by reader"),
            );
            for (val_ref, val_read) in ref_vector.iter().zip(read_vector.iter()) {
                assert_eq!(val_ref, *val_read);
            }
        }
    }
}
