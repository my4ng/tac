use memmap2::Mmap;

use std::fs::File;
use std::io::prelude::*;
use std::io::{BufWriter, IsTerminal, Result};

const SEARCH: u8 = b'\n';
const MAX_BUF_SIZE: usize = 4 * 1024 * 1024; // 4 MiB

#[cfg_attr(
    target_family = "unix",
    allow(unreachable_code),
    allow(unused_mut),
    allow(unused_variables)
)]
pub fn reverse_file(path: &str, force_flush: bool) -> Result<()> {
    let mut temp_path = None;
    {
        let mmap;
        let mut buf;
        let bytes = match path {
            #[cfg_attr(not(target_family = "unix"), allow(unused_labels))]
            "-" => 'stdin: {
                // Depending on what the STDIN fd actually points to, it may still be possible to
                // mmap the input (e.g. in case of `tac - < foo.txt`).
                #[cfg(target_family = "unix")]
                {
                    let stdin = std::io::stdin();
                    mmap = unsafe { Mmap::map(&stdin)? };
                    break 'stdin &mmap[..];
                }

                // We unfortunately need to buffer the entirety of the stdin input first;
                // we try to do so purely in memory but will switch to a backing file if
                // the input exceeds MAX_BUF_SIZE.
                buf = vec![0; MAX_BUF_SIZE];
                let mut reader = std::io::stdin();
                let mut total_read = 0;

                // Once/if we switch to a file-backed buffer, this will contain the handle.
                loop {
                    let bytes_read = reader.read(&mut buf[total_read..])?;
                    if bytes_read == 0 {
                        break &buf[0..total_read];
                    }
                    total_read += bytes_read;

                    if total_read == MAX_BUF_SIZE {
                        temp_path =
                            Some(std::env::temp_dir().join(format!(".tac-{}", std::process::id())));
                        let mut temp_file = File::create(temp_path.as_ref().unwrap())?;
                        // Write everything we've read so far
                        temp_file.write_all(&buf)?;
                        // Copy remaining bytes directly from stdin
                        std::io::copy(&mut reader, &mut temp_file)?;
                        mmap = unsafe { Mmap::map(&temp_file)? };
                        break &mmap[..];
                    }
                }
            }
            _ => {
                let file = File::open(path)?;
                mmap = unsafe { Mmap::map(&file)? };
                &mmap[..]
            }
        };

        let output = std::io::stdout();
        let mut output = output.lock();
        let mut buffered_output;

        let mut output: &mut dyn Write = if force_flush || output.is_terminal() {
            &mut output
        } else {
            buffered_output = BufWriter::new(output);
            &mut buffered_output
        };

        if bytes.is_empty() {
            // Do nothing. This avoids an underflow in the search functions which expect there to
            // be at least one byte.
        } else {
            search_auto(bytes, &mut output)?;
        }
    }

    if let Some(ref path) = temp_path.as_ref() {
        // This should never fail unless we've somehow kept a handle open to it
        if let Err(e) = std::fs::remove_file(path) {
            eprintln!(
                "Error: failed to remove temporary file {}\n{}",
                path.display(),
                e
            )
        };
    }

    Ok(())
}

fn search_auto(bytes: &[u8], mut output: &mut dyn Write) -> Result<()> {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if is_x86_feature_detected!("avx2")
        && is_x86_feature_detected!("lzcnt")
        && is_x86_feature_detected!("bmi2")
    {
        return unsafe { search256(bytes, &mut output) };
    }

    #[cfg(target_arch = "aarch64")]
    if std::arch::is_aarch64_feature_detected!("neon") {
        return search128(bytes, &mut output);
    }

    search(bytes, &mut output)
}

/// This is the default, naïve byte search
fn search(bytes: &[u8], output: &mut dyn Write) -> Result<()> {
    let mut last_printed = bytes.len();

    for index in (0..bytes.len()).rev() {
        if bytes[index] == SEARCH {
            output.write_all(&bytes[index + 1..last_printed])?;
            last_printed = index + 1;
        }
    }
    output.write_all(&bytes[..last_printed])?;
    Ok(())
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))]
#[inline(always)]
/// Search a range index-by-index and write to `output` when a match is found. Primarily used to
/// search before/after the aligned portion of a range.
fn slow_search_and_print(
    bytes: &[u8],
    start: usize,
    end: usize,
    stop: &mut usize,
    output: &mut dyn Write,
) -> Result<()> {
    for index in (start..end).rev() {
        if bytes[index] == SEARCH {
            output.write_all(&bytes[index + 1..*stop])?;
            *stop = index + 1;
        }
    }

    Ok(())
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
#[target_feature(enable = "lzcnt")]
#[target_feature(enable = "bmi2")]
/// This is an AVX2-optimized newline search function that searches a 32-byte (256-bit) window
/// instead of scanning character-by-character (once aligned). This is a *safe* function, but must
/// be adorned with `unsafe` to guarantee it's not called without first checking for AVX2 support.
///
/// We need to explicitly enable lzcnt support for u32::leading_zeros() to use the `lzcnt`
/// instruction instead of an extremely slow combination of branching + BSR.
///
/// BMI2 is explicitly opted into to inline the BZHI instruction; otherwise a call to the intrinsic
/// function is added and not inlined.
unsafe fn search256(bytes: &[u8], mut output: &mut dyn Write) -> Result<()> {
    use core::arch::x86_64::*;

    const ALIGNMENT: usize = std::mem::align_of::<__m256i>();

    let ptr = bytes.as_ptr();
    let len = bytes.len();
    let mut last_printed = len;
    let mut remaining = len;

    // We should only use 32-byte (256-bit) aligned reads w/ AVX2 intrinsics.
    // Search unaligned bytes via slow method so subsequent haystack reads are always aligned.
    // Guaranteed to have at least two aligned blocks
    if len >= ALIGNMENT * 3 - 1 {
        // Regardless of whether or not the base pointer is aligned to a 32-byte address, we are
        // reading from an arbitrary offset (determined by the length of the lines) and so we must
        // first calculate a safe place to begin using SIMD operations from.
        let align_offset = unsafe { ptr.add(len) }.align_offset(ALIGNMENT);
        if align_offset != 0 {
            let aligned_index = len + align_offset - ALIGNMENT;
            debug_assert!(aligned_index < len && aligned_index > 0);
            debug_assert!((ptr as usize + aligned_index) % ALIGNMENT == 0,);

            // eprintln!("Unoptimized search from {} to {}", aligned_index, last_printed);
            slow_search_and_print(bytes, aligned_index, len, &mut last_printed, &mut output)?;
            remaining = aligned_index;
        } else {
            // `bytes` end in an aligned block, no need to offset
            debug_assert!((ptr as usize + len) % ALIGNMENT == 0);
        }

        let pattern256 = unsafe { _mm256_set1_epi8(SEARCH as i8) };
        while remaining >= 64 {
            let window_end_offset = remaining;
            unsafe {
                remaining -= 32;
                let search256 = _mm256_load_si256(ptr.add(remaining) as *const __m256i);
                let result256 = _mm256_cmpeq_epi8(search256, pattern256);
                let mut matches = _mm256_movemask_epi8(result256) as u32 as u64;

                // Partially unroll this loop by repeating the above again before handling results
                remaining -= 32;
                let search256 = _mm256_load_si256(ptr.add(remaining) as *const __m256i);
                let result256 = _mm256_cmpeq_epi8(search256, pattern256);
                matches = (matches << 32) | _mm256_movemask_epi8(result256) as u32 as u64;

                while matches != 0 {
                    // We would count *trailing* zeroes to find new lines in reverse order, but the
                    // result mask is in little endian (reversed) order, so we do the very
                    // opposite.
                    // core::intrinsics::ctlz() is not stabilized, but `u64::leading_zeros()` will
                    // use it directly if the lzcnt or bmi1 features are enabled.
                    let leading = matches.leading_zeros();
                    let offset = window_end_offset - leading as usize;

                    output.write_all(&bytes[offset..last_printed])?;
                    last_printed = offset;

                    // Clear this match from the matches bitset. The equivalent:
                    // matches &= !(1 << (64 - leading - 1));
                    matches = _bzhi_u64(matches, 63 - leading);
                }
            }
        }
    }

    if remaining != 0 {
        // eprintln!("Unoptimized end search from {} to {}", 0, index);
        slow_search_and_print(bytes, 0, remaining, &mut last_printed, &mut output)?;
    }

    // Regardless of whether or not `index` is zero, as this is predicated on `last_printed`
    output.write_all(&bytes[..last_printed])?;

    Ok(())
}

#[cfg(target_arch = "aarch64")]
/// This is a NEON/AdvSIMD-optimized newline search function that searches a 16-byte (128-bit) window
/// instead of scanning character-by-character (once aligned).
fn search128(bytes: &[u8], mut output: &mut dyn Write) -> Result<()> {
    use core::arch::aarch64::*;

    let ptr = bytes.as_ptr();
    let mut last_printed = bytes.len();
    let mut index = last_printed - 1;

    if index >= 64 {
        // ARMv8 loads do not have alignment *requirements*, but there can be performance penalties
        // (e.g. seems to be about 2% slowdown on Cortex-A72 with a 500MB file) so let's align.
        // Search unaligned bytes via slow method so subsequent haystack reads are always aligned.
        let align_offset = unsafe { ptr.add(index).align_offset(16) };
        let aligned_index = index + align_offset - 16;

        // eprintln!("Unoptimized search from {} to {}", aligned_index, last_printed);
        slow_search_and_print(
            bytes,
            aligned_index,
            last_printed,
            &mut last_printed,
            &mut output,
        )?;
        index = aligned_index;

        let pattern128 = unsafe { vdupq_n_u8(SEARCH) };
        while index >= 64 {
            let window_end_offset = index;
            unsafe {
                index -= 16;
                let window = ptr.add(index);
                let search128 = vld1q_u8(window);
                let result128_0 = vceqq_u8(search128, pattern128);

                index -= 16;
                let window = ptr.add(index);
                let search128 = vld1q_u8(window);
                let result128_1 = vceqq_u8(search128, pattern128);

                index -= 16;
                let window = ptr.add(index);
                let search128 = vld1q_u8(window);
                let result128_2 = vceqq_u8(search128, pattern128);

                index -= 16;
                let window = ptr.add(index);
                let search128 = vld1q_u8(window);
                let result128_3 = vceqq_u8(search128, pattern128);

                // Bulk movemask as described in
                // https://branchfree.org/2019/04/01/fitting-my-head-through-the-arm-holes/
                let mut matches = {
                    let bit_mask: uint8x16_t = std::mem::transmute([
                        0x01u8, 0x02, 0x4, 0x8, 0x10, 0x20, 0x40, 0x80, 0x01, 0x02, 0x4, 0x8, 0x10,
                        0x20, 0x40, 0x80,
                    ]);
                    let t0 = vandq_u8(result128_3, bit_mask);
                    let t1 = vandq_u8(result128_2, bit_mask);
                    let t2 = vandq_u8(result128_1, bit_mask);
                    let t3 = vandq_u8(result128_0, bit_mask);
                    let sum0 = vpaddq_u8(t0, t1);
                    let sum1 = vpaddq_u8(t2, t3);
                    let sum0 = vpaddq_u8(sum0, sum1);
                    let sum0 = vpaddq_u8(sum0, sum0);
                    vgetq_lane_u64(vreinterpretq_u64_u8(sum0), 0)
                };

                while matches != 0 {
                    // We would count *trailing* zeroes to find new lines in reverse order, but the
                    // result mask is in little endian (reversed) order, so we do the very
                    // opposite.
                    let leading = matches.leading_zeros();
                    let offset = window_end_offset - leading as usize;

                    output.write_all(&bytes[offset..last_printed])?;
                    last_printed = offset;

                    // Clear this match from the matches bitset.
                    matches &= !(1 << (64 - leading - 1));
                }
            }
        }
    }

    if index != 0 {
        // eprintln!("Unoptimized end search from {} to {}", 0, index);
        slow_search_and_print(bytes, 0, index, &mut last_printed, &mut output)?;
    }

    // Regardless of whether or not `index` is zero, as this is predicated on `last_printed`
    output.write_all(&bytes[0..last_printed])?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[cfg(target_os = "linux")]
    #[test]
    fn test_x86_simd() {
        let mut file = File::open("/dev/urandom").unwrap();
        let mut buffer = [0; 1023];
        for _ in 0..100_000 {
            test(&buffer);
            file.read_exact(&mut buffer).unwrap();
        }

        fn test(buf: &[u8]) {
            let mut slow_result = Vec::new();
            let mut simd_result = Vec::new();
            search(buf, &mut slow_result).unwrap();
            unsafe { search256(buf, &mut simd_result).unwrap() };
            assert_eq!(slow_result, simd_result);
        }
    }
}
