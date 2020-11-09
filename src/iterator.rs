use crate::*;
use std::io::Seek;
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

struct SeekTrajectoryIterator<'a, T> {
    prev_position: u64,
    trajectory: &'a mut T,
    item: Rc<Frame>,
    has_error: bool,
}

impl<'a, T: Trajectory + Seek> SeekTrajectoryIterator<'a, T> {
    /// Private function to handle reading the next frame in sequence without having to worry about errors
    fn next_inner(&mut self) -> Result<Rc<Frame>> {
        let frame: &mut Frame = match Rc::get_mut(&mut self.item) {
            Some(item) => item,
            None => {
                // caller kept frame. Create new one
                self.item = Rc::new(Frame::with_capacity(self.item.num_atoms));
                Rc::get_mut(&mut self.item).unwrap()
            }
        };

        self.trajectory.read(frame)?;

        Ok(Rc::clone(&self.item))
    }

    /// Private function to finalise iteration
    fn finalise(&mut self) -> Option<<Self as Iterator>::Item> {
        // Seek back to where we were
        self.trajectory
            .seek(std::io::SeekFrom::Start(self.prev_position))
            .expect("Could not seek back to original position");
        None
    }
}

impl<'a, T: Trajectory + Seek> Iterator for SeekTrajectoryIterator<'a, T> {
    type Item = Result<Rc<Frame>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.has_error {
            return self.finalise();
        }

        match self.next_inner() {
            Ok(f) => Some(Ok(f)),
            Err(e) if e.is_eof() => self.finalise(),
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
}
