use std::collections::HashMap;

const EPS: f64 = 1e-14;

pub(crate) fn solve_sparse_gmres_ilu0(
    n: usize,
    triplets: &[(usize, usize, f64)],
    rhs: &[f64],
    tol: f64,
    max_iter: usize,
    restart: usize,
) -> Option<Vec<f64>> {
    if n == 0 {
        return Some(Vec::new());
    }
    if rhs.len() != n || restart == 0 || max_iter == 0 {
        return None;
    }

    let rows = build_row_maps(n, triplets);
    let precond = ilu0_factor(&rows)?;

    let mut x = vec![0.0_f64; n];
    let b_norm = l2_norm(rhs).max(EPS);
    let mut total_iter = 0usize;

    while total_iter < max_iter {
        let ax = matvec(&rows, &x);
        let mut r = vec_sub(rhs, &ax);
        r = apply_preconditioner(&precond, &r)?;
        let beta = l2_norm(&r);
        if beta / b_norm < tol {
            return Some(x);
        }

        let mut v = vec![vec![0.0_f64; n]; restart + 1];
        for (i, val) in r.iter().enumerate() {
            v[0][i] = *val / beta;
        }

        let mut h = vec![vec![0.0_f64; restart]; restart + 1];
        let mut cs = vec![0.0_f64; restart];
        let mut sn = vec![0.0_f64; restart];
        let mut g = vec![0.0_f64; restart + 1];
        g[0] = beta;

        let mut last_col = 0usize;
        let mut converged = false;
        for j in 0..restart {
            if total_iter >= max_iter {
                break;
            }
            total_iter += 1;
            last_col = j;

            let av = matvec(&rows, &v[j]);
            let mut w = apply_preconditioner(&precond, &av)?;

            for i in 0..=j {
                h[i][j] = dot(&w, &v[i]);
                axpy(&mut w, -h[i][j], &v[i]);
            }
            h[j + 1][j] = l2_norm(&w);
            if h[j + 1][j] > EPS {
                for (dst, src) in v[j + 1].iter_mut().zip(w.iter()) {
                    *dst = *src / h[j + 1][j];
                }
            }

            for i in 0..j {
                let temp = cs[i] * h[i][j] + sn[i] * h[i + 1][j];
                h[i + 1][j] = -sn[i] * h[i][j] + cs[i] * h[i + 1][j];
                h[i][j] = temp;
            }

            let (c, s) = givens(h[j][j], h[j + 1][j]);
            cs[j] = c;
            sn[j] = s;
            h[j][j] = c * h[j][j] + s * h[j + 1][j];
            h[j + 1][j] = 0.0;

            let g_next = -s * g[j];
            g[j] = c * g[j];
            g[j + 1] = g_next;

            let rel_res = g[j + 1].abs() / b_norm;
            if rel_res < tol {
                let y = back_substitute(&h, &g, j)?;
                update_solution(&mut x, &v, &y);
                converged = true;
                break;
            }
        }

        if converged {
            return Some(x);
        }

        let y = back_substitute(&h, &g, last_col)?;
        update_solution(&mut x, &v, &y);
    }

    let ax = matvec(&rows, &x);
    let r = vec_sub(rhs, &ax);
    if l2_norm(&r) / b_norm < tol {
        Some(x)
    } else {
        None
    }
}

#[derive(Debug, Clone)]
struct Ilu0Preconditioner {
    l_rows: Vec<Vec<(usize, f64)>>,
    u_rows: Vec<Vec<(usize, f64)>>,
    u_diag: Vec<f64>,
}

fn build_row_maps(n: usize, triplets: &[(usize, usize, f64)]) -> Vec<HashMap<usize, f64>> {
    let mut rows = vec![HashMap::<usize, f64>::new(); n];
    for &(row, col, val) in triplets {
        if row >= n || col >= n {
            continue;
        }
        *rows[row].entry(col).or_insert(0.0) += val;
    }
    rows
}

fn ilu0_factor(rows: &[HashMap<usize, f64>]) -> Option<Ilu0Preconditioner> {
    let n = rows.len();
    let mut l_maps = vec![HashMap::<usize, f64>::new(); n];
    let mut u_maps = vec![HashMap::<usize, f64>::new(); n];

    for i in 0..n {
        let mut cols: Vec<usize> = rows[i].keys().copied().collect();
        cols.sort_unstable();

        for &j in cols.iter().filter(|&&c| c < i) {
            let mut sum = *rows[i].get(&j).unwrap_or(&0.0);
            for (&k, &lik) in &l_maps[i] {
                if k >= j {
                    continue;
                }
                if let Some(&ukj) = u_maps[k].get(&j) {
                    sum -= lik * ukj;
                }
            }
            let ujj = *u_maps[j].get(&j)?;
            if ujj.abs() < EPS {
                return None;
            }
            l_maps[i].insert(j, sum / ujj);
        }

        for &j in cols.iter().filter(|&&c| c >= i) {
            let mut sum = *rows[i].get(&j).unwrap_or(&0.0);
            for (&k, &lik) in &l_maps[i] {
                if k >= i {
                    continue;
                }
                if let Some(&ukj) = u_maps[k].get(&j) {
                    sum -= lik * ukj;
                }
            }
            u_maps[i].insert(j, sum);
        }

        let diag = u_maps[i].get(&i).copied().unwrap_or(0.0);
        if diag.abs() < EPS {
            u_maps[i].insert(i, if diag >= 0.0 { 1e-8 } else { -1e-8 });
        }
    }

    let mut l_rows = vec![Vec::<(usize, f64)>::new(); n];
    let mut u_rows = vec![Vec::<(usize, f64)>::new(); n];
    let mut u_diag = vec![0.0_f64; n];

    for i in 0..n {
        let mut l_vec: Vec<(usize, f64)> = l_maps[i]
            .iter()
            .filter_map(|(&j, &v)| if j < i { Some((j, v)) } else { None })
            .collect();
        l_vec.sort_by_key(|(j, _)| *j);
        l_rows[i] = l_vec;

        let mut u_vec: Vec<(usize, f64)> = u_maps[i]
            .iter()
            .filter_map(|(&j, &v)| if j > i { Some((j, v)) } else { None })
            .collect();
        u_vec.sort_by_key(|(j, _)| *j);
        u_rows[i] = u_vec;
        u_diag[i] = *u_maps[i].get(&i)?;
    }

    Some(Ilu0Preconditioner {
        l_rows,
        u_rows,
        u_diag,
    })
}

fn apply_preconditioner(precond: &Ilu0Preconditioner, rhs: &[f64]) -> Option<Vec<f64>> {
    let n = rhs.len();
    let mut y = vec![0.0_f64; n];
    for i in 0..n {
        let mut sum = rhs[i];
        for &(j, lij) in &precond.l_rows[i] {
            sum -= lij * y[j];
        }
        y[i] = sum;
    }

    let mut x = vec![0.0_f64; n];
    for i in (0..n).rev() {
        let mut sum = y[i];
        for &(j, uij) in &precond.u_rows[i] {
            sum -= uij * x[j];
        }
        let diag = precond.u_diag[i];
        if diag.abs() < EPS {
            return None;
        }
        x[i] = sum / diag;
    }
    Some(x)
}

fn matvec(rows: &[HashMap<usize, f64>], x: &[f64]) -> Vec<f64> {
    rows.iter()
        .map(|row| row.iter().map(|(j, aij)| aij * x[*j]).sum())
        .collect()
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn l2_norm(v: &[f64]) -> f64 {
    dot(v, v).sqrt()
}

fn axpy(y: &mut [f64], alpha: f64, x: &[f64]) {
    for (dst, src) in y.iter_mut().zip(x.iter()) {
        *dst += alpha * src;
    }
}

fn vec_sub(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b.iter()).map(|(x, y)| x - y).collect()
}

fn givens(a: f64, b: f64) -> (f64, f64) {
    if b.abs() < EPS {
        (1.0, 0.0)
    } else if a.abs() < EPS {
        (0.0, 1.0)
    } else {
        let r = (a * a + b * b).sqrt();
        (a / r, b / r)
    }
}

fn back_substitute(h: &[Vec<f64>], g: &[f64], j_max: usize) -> Option<Vec<f64>> {
    let mut y = vec![0.0_f64; j_max + 1];
    for i in (0..=j_max).rev() {
        let mut sum = g[i];
        for (k, &h_ik) in h[i].iter().enumerate().skip(i + 1).take(j_max - i) {
            sum -= h_ik * y[k];
        }
        let diag = h[i][i];
        if diag.abs() < EPS {
            return None;
        }
        y[i] = sum / diag;
    }
    Some(y)
}

fn update_solution(x: &mut [f64], basis: &[Vec<f64>], y: &[f64]) {
    for (i, &coef) in y.iter().enumerate() {
        for (dst, &vij) in x.iter_mut().zip(basis[i].iter()) {
            *dst += coef * vij;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_gmres_ilu0_solves_small_system() {
        // [ 4 -1  0 ] [x0]   [15]
        // [-1  4 -1 ] [x1] = [10]
        // [ 0 -1  3 ] [x2]   [10]
        let triplets = vec![
            (0, 0, 4.0),
            (0, 1, -1.0),
            (1, 0, -1.0),
            (1, 1, 4.0),
            (1, 2, -1.0),
            (2, 1, -1.0),
            (2, 2, 3.0),
        ];
        let rhs = vec![15.0, 10.0, 10.0];
        let solution = super::solve_sparse_gmres_ilu0(3, &triplets, &rhs, 1e-10, 100, 10)
            .expect("gmres should converge");
        assert!((solution[0] - 5.0).abs() < 1e-6);
        assert!((solution[1] - 5.0).abs() < 1e-6);
        assert!((solution[2] - 5.0).abs() < 1e-6);
    }
}
