use std::cmp::Ordering;
use std::collections::HashMap;
use std::error::Error;
use std::io::{Cursor, Seek, SeekFrom, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use crc32fast::Hasher;

const DATA_URI_PREFIX: &str = "data:audio/vnd.shazam.sig;base64,";

pub struct FrequencyPeak {
    pub fft_pass_number: u32,
    pub peak_magnitude: u16,
    pub corrected_peak_frequency_bin: u16,
    pub sample_rate_hz: u32,
}

impl FrequencyPeak {
    pub fn get_frequency_hz(self: &Self) -> f32 {
        self.corrected_peak_frequency_bin as f32
            * (self.sample_rate_hz as f32 / 2.0 / 1024.0 / 64.0)
    }

    pub fn get_amplitude_pcm(self: &Self) -> f32 {
        (((self.peak_magnitude as f32 - 6144.0) / 1477.3).exp() * ((1 << 17) as f32) / 2.0).sqrt()
            / 1024.0
    }

    pub fn get_seconds(self: &Self) -> f32 {
        (self.fft_pass_number as f32 * 128.0) / self.sample_rate_hz as f32
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Clone, Copy)]
pub enum FrequencyBand {
    _250_520 = 0,
    _520_1450 = 1,
    _1450_3500 = 2,
    _3500_5500 = 3,
}

impl Ord for FrequencyBand {
    fn cmp(&self, other: &Self) -> Ordering {
        (*self as i32).cmp(&(*other as i32))
    }
}

impl PartialOrd for FrequencyBand {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some((*self as i32).cmp(&(*other as i32)))
    }
}

struct RawSignatureHeader {
    magic1: u32,
    crc32: u32,
    size_minus_header: u32,
    magic2: u32,
    _void1: [u32; 3],
    shifted_sample_rate_id: u32,
    _void2: [u32; 2],
    number_samples_plus_divided_sample_rate: u32,
    _fixed_value: u32,
}

pub struct DecodedSignature {
    pub sample_rate_hz: u32,
    pub number_samples: u32,
    pub frequency_band_to_sound_peaks: HashMap<FrequencyBand, Vec<FrequencyPeak>>,
}

impl DecodedSignature {
    pub fn decode_from_binary(data: &[u8]) -> Result<Self, Box<dyn Error>> {
        assert!(data.len() > 48 + 8);
        let mut cursor = Cursor::new(data);

        let header = RawSignatureHeader {
            magic1: cursor.read_u32::<LittleEndian>()?,
            crc32: cursor.read_u32::<LittleEndian>()?,
            size_minus_header: cursor.read_u32::<LittleEndian>()?,
            magic2: cursor.read_u32::<LittleEndian>()?,
            _void1: [
                cursor.read_u32::<LittleEndian>()?,
                cursor.read_u32::<LittleEndian>()?,
                cursor.read_u32::<LittleEndian>()?,
            ],
            shifted_sample_rate_id: cursor.read_u32::<LittleEndian>()?,
            _void2: [
                cursor.read_u32::<LittleEndian>()?,
                cursor.read_u32::<LittleEndian>()?,
            ],
            number_samples_plus_divided_sample_rate: cursor.read_u32::<LittleEndian>()?,
            _fixed_value: cursor.read_u32::<LittleEndian>()?,
        };

        let mut hasher = Hasher::new();
        hasher.update(&data[8..]);
        assert_eq!(header.magic1, 0xcafe2580);
        assert_eq!(header.size_minus_header as usize, data.len() - 48);
        assert_eq!(header.crc32, hasher.finalize());
        assert_eq!(header.magic2, 0x94119c00);

        let sample_rate_hz: u32 = match header.shifted_sample_rate_id >> 27 {
            1 => 8000,
            2 => 11025,
            3 => 16000,
            4 => 32000,
            5 => 44100,
            6 => 48000,
            _ => {
                panic!("Invalid sample rate in decoded Shazam packet");
            }
        };

        let number_samples: u32 =
            header.number_samples_plus_divided_sample_rate - (sample_rate_hz as f32 * 0.24) as u32;

        assert_eq!(cursor.read_u32::<LittleEndian>()?, 0x40000000);
        assert_eq!(cursor.read_u32::<LittleEndian>()? as usize, data.len() - 48);

        let mut frequency_band_to_sound_peaks: HashMap<FrequencyBand, Vec<FrequencyPeak>> =
            HashMap::new();

        while cursor.position() < data.len() as u64 {
            let frequency_band_id = cursor.read_u32::<LittleEndian>()?;
            let frequency_peaks_size = cursor.read_u32::<LittleEndian>()?;

            let frequency_peaks_padding = (4 - frequency_peaks_size % 4) % 4;

            let mut frequency_peaks_cursor = Cursor::new(
                &data[cursor.position() as usize
                    ..(cursor.position() as u32 + frequency_peaks_size) as usize],
            );

            let frequency_band = match frequency_band_id - 0x60030040 {
                0 => FrequencyBand::_250_520,
                1 => FrequencyBand::_520_1450,
                2 => FrequencyBand::_1450_3500,
                3 => FrequencyBand::_3500_5500,
                _ => {
                    panic!("Invalid frequency band in decoded Shazam packet");
                }
            };

            let mut fft_pass_number: u32 = 0;

            while frequency_peaks_cursor.position() < frequency_peaks_size as u64 {
                let fft_pass_offset = frequency_peaks_cursor.read_u8()?;

                match fft_pass_offset {
                    0xff => {
                        fft_pass_number = frequency_peaks_cursor.read_u32::<LittleEndian>()?;
                    }
                    _ => {
                        fft_pass_number += fft_pass_offset as u32;

                        if !frequency_band_to_sound_peaks.contains_key(&frequency_band) {
                            frequency_band_to_sound_peaks.insert(frequency_band, vec![]);
                        }

                        frequency_band_to_sound_peaks
                            .get_mut(&frequency_band)
                            .unwrap()
                            .push(FrequencyPeak {
                                fft_pass_number: fft_pass_number,
                                peak_magnitude: frequency_peaks_cursor
                                    .read_u16::<LittleEndian>()?,
                                corrected_peak_frequency_bin: frequency_peaks_cursor
                                    .read_u16::<LittleEndian>()?,
                                sample_rate_hz: sample_rate_hz,
                            });
                    }
                };
            }

            cursor.seek(SeekFrom::Current(
                (frequency_peaks_size + frequency_peaks_padding) as i64,
            ))?;
        }

        // Return the decoded object

        Ok(DecodedSignature {
            sample_rate_hz: sample_rate_hz,
            number_samples: number_samples,
            frequency_band_to_sound_peaks: frequency_band_to_sound_peaks,
        })
    }

    // pub fn decode_from_uri(uri: &str) -> Result<Self, Box<dyn Error>> {
    //     assert!(uri.starts_with(DATA_URI_PREFIX));

    //     Ok(DecodedSignature::decode_from_binary(&base64::decode(
    //         &uri[DATA_URI_PREFIX.len()..],
    //     )?)?)
    // }

    pub fn encode_to_binary(self: &Self) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut cursor = Cursor::new(vec![]);

        cursor.write_u32::<LittleEndian>(0xcafe2580)?;
        cursor.write_u32::<LittleEndian>(0)?;
        cursor.write_u32::<LittleEndian>(0)?;
        cursor.write_u32::<LittleEndian>(0x94119c00)?;
        cursor.write_u32::<LittleEndian>(0)?;
        cursor.write_u32::<LittleEndian>(0)?;
        cursor.write_u32::<LittleEndian>(0)?;
        cursor.write_u32::<LittleEndian>(
            match self.sample_rate_hz {
                8000 => 1,
                11025 => 2,
                16000 => 3,
                32000 => 4,
                44100 => 5,
                48000 => 6,
                _ => {
                    panic!("Invalid sample rate passed when encoding Shazam packet");
                }
            } << 27,
        )?; // shifted_sample_rate_id
        cursor.write_u32::<LittleEndian>(0)?; // void2
        cursor.write_u32::<LittleEndian>(0)?;
        cursor.write_u32::<LittleEndian>(
            self.number_samples + (self.sample_rate_hz as f32 * 0.24) as u32,
        )?; // number_samples_plus_divided_sample_rate
        cursor.write_u32::<LittleEndian>((15 << 19) + 0x40000)?; // fixed_value

        cursor.write_u32::<LittleEndian>(0x40000000)?;
        cursor.write_u32::<LittleEndian>(0)?; // size_minus_header - Will write later

        let mut sorted_iterator: Vec<_> = self.frequency_band_to_sound_peaks.iter().collect();
        sorted_iterator.sort_by(|x, y| x.0.cmp(&y.0));

        for (frequency_band, frequency_peaks) in sorted_iterator {
            let mut peaks_cursor = Cursor::new(vec![]);

            let mut fft_pass_number = 0;

            for frequency_peak in frequency_peaks {
                assert!(frequency_peak.fft_pass_number >= fft_pass_number);

                if frequency_peak.fft_pass_number - fft_pass_number >= 255 {
                    peaks_cursor.write_u8(0xff)?;
                    peaks_cursor.write_u32::<LittleEndian>(frequency_peak.fft_pass_number)?;

                    fft_pass_number = frequency_peak.fft_pass_number;
                }

                peaks_cursor.write_u8((frequency_peak.fft_pass_number - fft_pass_number) as u8)?;

                peaks_cursor.write_u16::<LittleEndian>(frequency_peak.peak_magnitude)?;
                peaks_cursor
                    .write_u16::<LittleEndian>(frequency_peak.corrected_peak_frequency_bin)?;

                fft_pass_number = frequency_peak.fft_pass_number;
            }

            let peaks_buffer = peaks_cursor.into_inner();

            cursor.write_u32::<LittleEndian>(0x60030040 + *frequency_band as u32)?;
            cursor.write_u32::<LittleEndian>(peaks_buffer.len() as u32)?;
            cursor.write(&peaks_buffer)?;
            for _padding_index in 0..((4 - peaks_buffer.len() as u32 % 4) % 4) {
                cursor.write_u8(0)?;
            }
        }

        let buffer_size = cursor.position() as u32;

        cursor.seek(SeekFrom::Start(8))?;
        cursor.write_u32::<LittleEndian>(buffer_size - 48)?;

        cursor.seek(SeekFrom::Start(48 + 4))?;
        cursor.write_u32::<LittleEndian>(buffer_size - 48)?;

        cursor.seek(SeekFrom::Start(4))?;
        let mut hasher = Hasher::new();
        hasher.update(&cursor.get_ref()[8..]);
        cursor.write_u32::<LittleEndian>(hasher.finalize())?; // crc32

        Ok(cursor.into_inner())
    }

    // pub fn encode_to_uri(self: &Self) -> Result<String, Box<dyn Error>> {
    //     Ok(format!(
    //         "{}{}",
    //         DATA_URI_PREFIX,
    //         base64::encode(self.encode_to_binary()?)
    //     ))
    // }

    pub fn to_lure(self: &Self) -> Result<Vec<i16>, Box<dyn Error>> {
        let mut buffer: Vec<i16> = [0]
            .repeat((self.number_samples as f32 / self.sample_rate_hz as f32 * 16000.0) as usize);

        let samples_per_sine = (1.0 / (16000.0 / 2048.0) * 16000.0 * 0.5) as usize;

        for frequency_peaks in self.frequency_band_to_sound_peaks.values() {
            for frequency_peak in frequency_peaks {
                let start_offset_of_sine = (frequency_peak.get_seconds() * 16000.0) as usize;
                let end_offset_of_sine = start_offset_of_sine + samples_per_sine;
                let amplitude = frequency_peak.get_amplitude_pcm() as f32;
                let base_frequency = frequency_peak.get_frequency_hz() as f32;

                if end_offset_of_sine < self.number_samples as usize {
                    let frequencies: Vec<f32> = vec![base_frequency];

                    for frequency in frequencies {
                        for num_sample in start_offset_of_sine..end_offset_of_sine {
                            let soften_factor = match frequency == base_frequency {
                                true => 1.0,
                                false => 1.0 / 3.0,
                            };

                            buffer[num_sample] +=
                                ((2.0 * 3.14159265 * frequency * num_sample as f32 / 16000.0).sin()
                                    * amplitude
                                    * soften_factor) as i16;
                        }
                    }
                }
            }
        }

        Ok(buffer)
    }
}
