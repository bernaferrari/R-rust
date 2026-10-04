//! GNU's modified CACM347 sort, including its integer permutation temporaries.
#![forbid(unsafe_code)]

pub(super) fn sort(v: &mut [f64], a: &mut [f64], stack: &mut Vec<(usize, usize)>) {
    stack.clear();
    let mut i = 1;
    let mut j = v.len();
    let mut partition = true;
    loop {
        if i >= j {
            let Some(range) = stack.pop() else {
                return;
            };
            (i, j) = range;
            partition = false;
            continue;
        }
        if partition {
            let mut k = i;
            let middle = (j + i) / 2;
            let mut t = a[middle - 1].trunc();
            let mut pivot = v[middle - 1];
            if v[i - 1] > pivot {
                a[middle - 1] = a[i - 1];
                a[i - 1] = t;
                t = a[middle - 1].trunc();
                v[middle - 1] = v[i - 1];
                v[i - 1] = pivot;
                pivot = v[middle - 1];
            }
            let mut l = j;
            if v[j - 1] < pivot {
                a[middle - 1] = a[j - 1];
                a[j - 1] = t;
                t = a[middle - 1].trunc();
                v[middle - 1] = v[j - 1];
                v[j - 1] = pivot;
                pivot = v[middle - 1];
                if v[i - 1] > pivot {
                    a[middle - 1] = a[i - 1];
                    a[i - 1] = t;
                    v[middle - 1] = v[i - 1];
                    v[i - 1] = pivot;
                    pivot = v[middle - 1];
                }
            }
            loop {
                loop {
                    l -= 1;
                    if v[l - 1] <= pivot {
                        break;
                    }
                }
                let temporary = a[l - 1].trunc();
                let value = v[l - 1];
                loop {
                    k += 1;
                    if v[k - 1] >= pivot {
                        break;
                    }
                }
                if k > l {
                    break;
                }
                a[l - 1] = a[k - 1];
                a[k - 1] = temporary;
                v[l - 1] = v[k - 1];
                v[k - 1] = value;
            }
            if l - i > j - k {
                stack.push((i, l));
                i = k;
            } else {
                stack.push((k, j));
                j = l;
            }
        }
        if j - i > 10 || i == 1 {
            partition = true;
            continue;
        }
        // The partition sentinel bounds GNU's insertion loop. An explicit
        // zero bound also prevents underflow if this helper is changed later.
        i -= 1;
        loop {
            i += 1;
            if i == j {
                break;
            }
            let temporary = a[i].trunc();
            let value = v[i];
            if v[i - 1] <= value {
                continue;
            }
            let mut k = i;
            while k > 0 && value < v[k - 1] {
                a[k] = a[k - 1];
                v[k] = v[k - 1];
                k -= 1;
            }
            a[k] = temporary;
            v[k] = value;
        }
        let Some(range) = stack.pop() else {
            return;
        };
        (i, j) = range;
        partition = false;
    }
}
