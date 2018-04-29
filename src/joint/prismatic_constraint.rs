use na::{DVector, Real, Unit, Vector2};
use std::ops::Range;

use joint::{unit_constraint, JointConstraint};
use math::{AngularVector, Point, Vector, DIM, SPATIAL_DIM};
use object::{BodyHandle, BodySet};
use solver::helper;
use solver::{ConstraintSet, GenericNonlinearConstraint, IntegrationParameters,
             NonlinearConstraintGenerator};

pub struct PrismaticConstraint<N: Real> {
    b1: BodyHandle,
    b2: BodyHandle,
    anchor1: Point<N>,
    anchor2: Point<N>,
    axis1: Unit<Vector<N>>,
    axis2: Unit<Vector<N>>,
    lin_impulses: Vector<N>,
    ang_impulses: AngularVector<N>,
    limit_impulse: N,
    bilateral_ground_rng: Range<usize>,
    bilateral_rng: Range<usize>,

    min_offset: Option<N>,
    max_offset: Option<N>,
}

impl<N: Real> PrismaticConstraint<N> {
    pub fn new(
        b1: BodyHandle,
        b2: BodyHandle,
        anchor1: Point<N>,
        axis1: Unit<Vector<N>>,
        anchor2: Point<N>,
        axis2: Unit<Vector<N>>,
    ) -> Self {
        let min_offset = None;
        let max_offset = None;

        PrismaticConstraint {
            b1,
            b2,
            anchor1,
            anchor2,
            axis1,
            axis2,
            lin_impulses: Vector::zeros(),
            ang_impulses: AngularVector::zeros(),
            limit_impulse: N::zero(),
            bilateral_ground_rng: 0..0,
            bilateral_rng: 0..0,
            min_offset,
            max_offset,
        }
    }

    pub fn min_offset(&self) -> Option<N> {
        self.min_offset
    }

    pub fn max_offset(&self) -> Option<N> {
        self.max_offset
    }

    pub fn disable_min_offset(&mut self) {
        self.min_offset = None;
    }

    pub fn disable_max_offset(&mut self) {
        self.max_offset = None;
    }

    pub fn enable_min_offset(&mut self, limit: N) {
        self.min_offset = Some(limit);
        self.assert_limits();
    }

    pub fn enable_max_offset(&mut self, limit: N) {
        self.max_offset = Some(limit);
        self.assert_limits();
    }

    fn assert_limits(&self) {
        if let (Some(min_offset), Some(max_offset)) = (self.min_offset, self.max_offset) {
            assert!(
                min_offset <= max_offset,
                "RevoluteJoint constraint limits: the min angle must be larger than (or equal to) the max angle.");
        }
    }
}

impl<N: Real> JointConstraint<N> for PrismaticConstraint<N> {
    fn num_velocity_constraints(&self) -> usize {
        (SPATIAL_DIM - 1) + 2
    }

    fn anchors(&self) -> (BodyHandle, BodyHandle) {
        (self.b1, self.b2)
    }

    fn velocity_constraints(
        &mut self,
        params: &IntegrationParameters<N>,
        bodies: &BodySet<N>,
        ext_vels: &DVector<N>,
        ground_j_id: &mut usize,
        j_id: &mut usize,
        jacobians: &mut [N],
        constraints: &mut ConstraintSet<N>,
    ) {
        let b1 = bodies.body_part(self.b1);
        let b2 = bodies.body_part(self.b2);

        /*
         *
         * Joint constraints.
         *
         */
        let pos1 = b1.position();
        let pos2 = b2.position();

        let anchor1 = pos1 * self.anchor1;
        let anchor2 = pos2 * self.anchor2;

        let assembly_id1 = b1.parent_companion_id();
        let assembly_id2 = b2.parent_companion_id();

        let first_bilateral_ground = constraints.velocity.bilateral_ground.len();
        let first_bilateral = constraints.velocity.bilateral.len();

        let axis1 = pos1 * self.axis1;
        let axis2 = pos2 * self.axis2;

        helper::restrict_relative_linear_velocity_to_axis(
            params,
            &b1,
            &b2,
            assembly_id1,
            assembly_id2,
            &anchor1,
            &anchor2,
            &axis1,
            &axis2,
            ext_vels,
            self.lin_impulses.as_slice(),
            0,
            ground_j_id,
            j_id,
            jacobians,
            constraints,
        );

        helper::cancel_relative_angular_velocity(
            params,
            &b1,
            &b2,
            assembly_id1,
            assembly_id2,
            &anchor1,
            &anchor2,
            ext_vels,
            &self.ang_impulses,
            DIM - 1,
            ground_j_id,
            j_id,
            jacobians,
            constraints,
        );

        /*
         *
         * Limit constraints.
         *
         */
        unit_constraint::build_linear_limits_velocity_constraint(
            params,
            &b1,
            &b2,
            assembly_id1,
            assembly_id2,
            &anchor1,
            &anchor2,
            &axis1,
            self.min_offset,
            self.max_offset,
            ext_vels,
            self.limit_impulse,
            SPATIAL_DIM - 1,
            ground_j_id,
            j_id,
            jacobians,
            constraints,
        );

        self.bilateral_ground_rng =
            first_bilateral_ground..constraints.velocity.bilateral_ground.len();
        self.bilateral_rng = first_bilateral..constraints.velocity.bilateral.len();
    }

    fn cache_impulses(&mut self, constraints: &ConstraintSet<N>) {
        for c in &constraints.velocity.bilateral_ground[self.bilateral_ground_rng.clone()] {
            if c.impulse_id < DIM - 1 {
                self.lin_impulses[c.impulse_id] = c.impulse;
            } else if c.impulse_id < SPATIAL_DIM - 1 {
                self.ang_impulses[c.impulse_id - DIM + 1] = c.impulse;
            } else {
                self.limit_impulse = c.impulse
            }
        }

        for c in &constraints.velocity.bilateral[self.bilateral_rng.clone()] {
            if c.impulse_id < DIM - 1 {
                self.lin_impulses[c.impulse_id] = c.impulse;
            } else if c.impulse_id < SPATIAL_DIM - 1 {
                self.ang_impulses[c.impulse_id - DIM + 1] = c.impulse;
            } else {
                self.limit_impulse = c.impulse
            }
        }
    }
}

impl<N: Real> NonlinearConstraintGenerator<N> for PrismaticConstraint<N> {
    fn num_position_constraints(&self, bodies: &BodySet<N>) -> usize {
        // FIXME: calling this at each iteration of the non-linear resolution is costly.
        if self.is_active(bodies) {
            3
        } else {
            0
        }
    }

    fn position_constraint(
        &self,
        params: &IntegrationParameters<N>,
        i: usize,
        bodies: &mut BodySet<N>,
        jacobians: &mut [N],
    ) -> Option<GenericNonlinearConstraint<N>> {
        let body1 = bodies.body_part(self.b1);
        let body2 = bodies.body_part(self.b2);

        let pos1 = body1.position();
        let pos2 = body2.position();

        let anchor1 = pos1 * self.anchor1;
        let anchor2 = pos2 * self.anchor2;

        if i == 0 {
            return helper::cancel_relative_rotation(
                params,
                &body1,
                &body2,
                &anchor1,
                &anchor2,
                &pos1.rotation,
                &pos2.rotation,
                jacobians,
            );
        } else if i == 1 {
            let axis1 = pos1 * self.axis1;
            let axis2 = pos2 * self.axis2;

            return helper::project_anchor_to_axis(
                params,
                &body1,
                &body2,
                &anchor1,
                &anchor2,
                &axis1,
                &axis2,
                jacobians,
            );
        } else if i == 2 {
            let axis1 = pos1 * self.axis1;

            return unit_constraint::build_linear_limits_position_constraint(
                params,
                &body1,
                &body2,
                &anchor1,
                &anchor2,
                &axis1,
                self.min_offset,
                self.max_offset,
                jacobians,
            );
        }

        return None;
    }
}
