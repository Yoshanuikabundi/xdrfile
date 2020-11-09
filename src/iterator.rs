use crate::*;
use std::rc::Rc;

impl IntoIterator for XTCTrajectory {
    type Item = Result<Rc<Frame>>;
    type IntoIter = TrajectoryIterator<XTCTrajectory>;

    fn into_iter(mut self) -> Self::IntoIter {
        let frame = match self.get_num_atoms() {
            Ok(num_atoms) => Frame::with_capacity(num_atoms),
            Err(_) => Frame::new(),
        };
        TrajectoryIterator {
            trajectory: self,
            item: Rc::new(frame),
            has_error: false,
        }
    }
}

impl IntoIterator for TRRTrajectory {
    type Item = Result<Rc<Frame>>;
    type IntoIter = TrajectoryIterator<TRRTrajectory>;

    fn into_iter(mut self) -> Self::IntoIter {
        let frame = match self.get_num_atoms() {
            Ok(num_atoms) => Frame::with_capacity(num_atoms),
            Err(_) => Frame::new(),
        };
        TrajectoryIterator {
            trajectory: self,
            item: Rc::new(frame),
            has_error: false,
        }
    }
}

/*
Iterator for trajectories. This iterator yields a Result<Frame, Error>
for each frame in the trajectory file and stops with yielding None once the
trajectory is finished. Also yields None after the first occurence of an error
*/
pub struct TrajectoryIterator<T> {
    trajectory: T,
    item: Rc<Frame>,
    has_error: bool,
}

impl<T> Iterator for TrajectoryIterator<T>
where
    T: Trajectory,
{
    type Item = Result<Rc<Frame>>;

    fn next(&mut self) -> Option<Self::Item> {
        // Reuse old frame
        if self.has_error {
            return None;
        }

        let item: &mut Frame = match Rc::get_mut(&mut self.item) {
            Some(item) => item,
            None => {
                // caller kept frame. Create new one
                self.item = Rc::new(Frame::with_capacity(self.item.num_atoms));
                Rc::get_mut(&mut self.item).expect("Could not get mutable access to new Rc")
            }
        };
        match self.trajectory.read(item) {
            Ok(()) => Some(Ok(Rc::clone(&self.item))),
            Err(e) if e.is_eof() => None,
            Err(e) => {
                self.has_error = true;
                Some(Err(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_xtc_trajectory_iterator() -> Result<()> {
        let traj = XTCTrajectory::open_read("tests/1l2y.xtc")?;
        let frames: Result<Vec<Rc<Frame>>> = traj.into_iter().collect();
        let frames = frames?;
        assert!(frames.len() == 38);
        assert!(frames[0].step == 1, frames[0].step);
        assert!(frames[37].step == 38);
        Ok(())
    }

    #[test]
    pub fn test_trr_trajectory_iterator() -> Result<()> {
        let traj = TRRTrajectory::open_read("tests/1l2y.trr")?;
        let frames: Result<Vec<Rc<Frame>>> = traj.into_iter().collect();
        let frames = frames?;
        assert!(frames.len() == 38);
        assert!(frames[0].step == 1, frames[0].step);
        assert!(frames[37].step == 38);
        Ok(())
    }

    #[test]
    pub fn test_iterators() -> Result<(), Box<dyn std::error::Error>> {
        let xtc_traj = XTCTrajectory::open_read("tests/1l2y.xtc")?;
        let trr_traj = TRRTrajectory::open_read("tests/1l2y.trr")?;

        for (xtc, trr) in xtc_traj.into_iter().zip(trr_traj) {
            let xtc = xtc?;
            let xtc = xtc.as_ref();
            let trr = trr?;
            let trr = trr.as_ref();
            assert_eq!(xtc.num_atoms, trr.num_atoms);
            assert_eq!(xtc.step, trr.step);
            assert_eq!(xtc.time, trr.time);
            assert_eq!(xtc.box_vector, trr.box_vector);
            for (xtc_xyz, trr_xyz) in xtc.coords.iter().zip(&trr.coords) {
                assert!(xtc_xyz[0] - trr_xyz[0] <= 1e-5);
                assert!(xtc_xyz[1] - trr_xyz[1] <= 1e-5);
                assert!(xtc_xyz[2] - trr_xyz[2] <= 1e-5);
            }
        }
        Ok(())
    }
}
