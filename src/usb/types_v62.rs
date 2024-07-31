#![allow(clippy::missing_safety_doc)]
#![allow(clippy::identity_op)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::erasing_op)]

#[doc = "Queue head block for hpm USB device"]
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Qhd {
    ptr: *mut u8,
}
unsafe impl Send for Qhd {}
unsafe impl Sync for Qhd {}
impl Qhd {
    #[inline(always)]
    pub const unsafe fn from_ptr(ptr: *mut ()) -> Self {
        Self { ptr: ptr as _ }
    }
    #[inline(always)]
    pub const fn as_ptr(&self) -> *mut () {
        self.ptr as _
    }
    #[doc = "Capabilities and characteristics"]
    #[inline(always)]
    pub const fn cap(self) -> common::Reg<regs::Cap, common::RW> {
        unsafe { common::Reg::from_ptr(self.ptr.add(0x0usize) as _) }
    }
    #[doc = "Current dtd address"]
    #[inline(always)]
    pub const fn cur_dtd(self) -> common::Reg<regs::CurDtd, common::RW> {
        unsafe { common::Reg::from_ptr(self.ptr.add(0x04usize) as _) }
    }
    #[doc = "Next dtd address and termination control"]
    #[inline(always)]
    pub const fn next_dtd(self) -> common::Reg<regs::NextDtd, common::RW> {
        unsafe { common::Reg::from_ptr(self.ptr.add(0x08usize) as _) }
    }
    #[doc = "Other fields in queue transfer descriptor"]
    #[inline(always)]
    pub const fn qtd_token(self) -> common::Reg<regs::QtdToken, common::RW> {
        unsafe { common::Reg::from_ptr(self.ptr.add(0x0cusize) as _) }
    }
    #[doc = "Buffer pointer"]
    #[inline(always)]
    pub const fn buffer(self, n: usize) -> common::Reg<regs::Buffer, common::RW> {
        assert!(n < 5usize);
        unsafe { common::Reg::from_ptr(self.ptr.add(0x10usize + n * 4usize) as _) }
    }
    #[doc = "Current offset in buffer"]
    #[inline(always)]
    pub const fn current_offset(self) -> common::Reg<regs::CurrentOffset, common::RW> {
        unsafe { common::Reg::from_ptr(self.ptr.add(0x10usize) as _) }
    }
    #[doc = "Buffer for setup packet"]
    #[inline(always)]
    pub const fn setup_buffer(self, n: usize) -> common::Reg<regs::SetupBuffer, common::RW> {
        assert!(n < 2usize);
        unsafe { common::Reg::from_ptr(self.ptr.add(0x28usize + n * 4usize) as _) }
    }
}
#[doc = "List of queue head blocks for hpm USB device"]
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct QhdList {
    ptr: *mut u8,
}
unsafe impl Send for QhdList {}
unsafe impl Sync for QhdList {}
impl QhdList {
    #[inline(always)]
    pub const unsafe fn from_ptr(ptr: *mut ()) -> Self {
        Self { ptr: ptr as _ }
    }
    #[inline(always)]
    pub const fn as_ptr(&self) -> *mut () {
        self.ptr as _
    }
    #[doc = "Queue head block for hpm USB device"]
    #[inline(always)]
    pub const fn qhd(self, n: usize) -> Qhd {
        assert!(n < 16usize);
        unsafe { Qhd::from_ptr(self.ptr.add(0x0usize + n * 64usize) as _) }
    }
}
#[doc = "Queue transfer descriptor for hpm USB device"]
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Qtd {
    ptr: *mut u8,
}
unsafe impl Send for Qtd {}
unsafe impl Sync for Qtd {}
impl Qtd {
    #[inline(always)]
    pub const unsafe fn from_ptr(ptr: *mut ()) -> Self {
        Self { ptr: ptr as _ }
    }
    #[inline(always)]
    pub const fn as_ptr(&self) -> *mut () {
        self.ptr as _
    }
    #[doc = "Next dtd address and termination control"]
    #[inline(always)]
    pub const fn next_dtd(self) -> common::Reg<regs::NextDtd, common::RW> {
        unsafe { common::Reg::from_ptr(self.ptr.add(0x0usize) as _) }
    }
    #[doc = "Other fields in queue transfer descriptor"]
    #[inline(always)]
    pub const fn qtd_token(self) -> common::Reg<regs::QtdToken, common::RW> {
        unsafe { common::Reg::from_ptr(self.ptr.add(0x04usize) as _) }
    }
    #[doc = "Buffer pointer"]
    #[inline(always)]
    pub const fn buffer(self, n: usize) -> common::Reg<regs::Buffer, common::RW> {
        assert!(n < 5usize);
        unsafe { common::Reg::from_ptr(self.ptr.add(0x08usize + n * 4usize) as _) }
    }
    #[doc = "Current offset in buffer"]
    #[inline(always)]
    pub const fn current_offset(self) -> common::Reg<regs::CurrentOffset, common::RW> {
        unsafe { common::Reg::from_ptr(self.ptr.add(0x08usize) as _) }
    }
    #[doc = "Number of bytes expected to transfer"]
    #[inline(always)]
    pub const fn expected_bytes(self) -> common::Reg<regs::ExpectedBytes, common::RW> {
        unsafe { common::Reg::from_ptr(self.ptr.add(0x1cusize) as _) }
    }
}
#[doc = "List of queue transfer descriptors for hpm USB device"]
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct QtdList {
    ptr: *mut u8,
}
unsafe impl Send for QtdList {}
unsafe impl Sync for QtdList {}
impl QtdList {
    #[inline(always)]
    pub const unsafe fn from_ptr(ptr: *mut ()) -> Self {
        Self { ptr: ptr as _ }
    }
    #[inline(always)]
    pub const fn as_ptr(&self) -> *mut () {
        self.ptr as _
    }
    #[doc = "Queue transfer descriptor for hpm USB device"]
    #[inline(always)]
    pub const fn qtd(self, n: usize) -> Qtd {
        assert!(n < 128usize);
        unsafe { Qtd::from_ptr(self.ptr.add(0x0usize + n * 32usize) as _) }
    }
}
pub mod common {
    use core::marker::PhantomData;
    #[derive(Copy, Clone, PartialEq, Eq)]
    pub struct RW;
    #[derive(Copy, Clone, PartialEq, Eq)]
    pub struct R;
    #[derive(Copy, Clone, PartialEq, Eq)]
    pub struct W;
    mod sealed {
        use super::*;
        pub trait Access {}
        impl Access for R {}
        impl Access for W {}
        impl Access for RW {}
    }
    pub trait Access: sealed::Access + Copy {}
    impl Access for R {}
    impl Access for W {}
    impl Access for RW {}
    pub trait Read: Access {}
    impl Read for RW {}
    impl Read for R {}
    pub trait Write: Access {}
    impl Write for RW {}
    impl Write for W {}
    #[derive(Copy, Clone, PartialEq, Eq)]
    pub struct Reg<T: Copy, A: Access> {
        ptr: *mut u8,
        phantom: PhantomData<*mut (T, A)>,
    }
    unsafe impl<T: Copy, A: Access> Send for Reg<T, A> {}
    unsafe impl<T: Copy, A: Access> Sync for Reg<T, A> {}
    impl<T: Copy, A: Access> Reg<T, A> {
        #[allow(clippy::missing_safety_doc)]
        #[inline(always)]
        pub const unsafe fn from_ptr(ptr: *mut T) -> Self {
            Self {
                ptr: ptr as _,
                phantom: PhantomData,
            }
        }
        #[inline(always)]
        pub const fn as_ptr(&self) -> *mut T {
            self.ptr as _
        }
    }
    impl<T: Copy, A: Read> Reg<T, A> {
        #[inline(always)]
        pub fn read(&self) -> T {
            unsafe { (self.ptr as *mut T).read_volatile() }
        }
    }
    impl<T: Copy, A: Write> Reg<T, A> {
        #[inline(always)]
        pub fn write_value(&self, val: T) {
            unsafe { (self.ptr as *mut T).write_volatile(val) }
        }
    }
    impl<T: Default + Copy, A: Write> Reg<T, A> {
        #[inline(always)]
        pub fn write<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
            let mut val = Default::default();
            let res = f(&mut val);
            self.write_value(val);
            res
        }
    }
    impl<T: Copy, A: Read + Write> Reg<T, A> {
        #[inline(always)]
        pub fn modify<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
            let mut val = self.read();
            let res = f(&mut val);
            self.write_value(val);
            res
        }
    }
}
pub mod regs {
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Buffer(pub u32);
    impl Buffer {
        #[doc = "4K aligned buffer pointer"]
        #[inline(always)]
        pub const fn buffer(&self) -> u32 {
            let val = (self.0 >> 12usize) & 0x000f_ffff;
            val as u32
        }
        #[doc = "4K aligned buffer pointer"]
        #[inline(always)]
        pub fn set_buffer(&mut self, val: u32) {
            self.0 = (self.0 & !(0x000f_ffff << 12usize)) | (((val as u32) & 0x000f_ffff) << 12usize);
        }
    }
    impl Default for Buffer {
        #[inline(always)]
        fn default() -> Buffer {
            Buffer(0)
        }
    }
    #[doc = "Capabilities and characteristics"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Cap(pub u32);
    impl Cap {
        #[doc = "Interrupt on setup packet"]
        #[inline(always)]
        pub const fn ios(&self) -> bool {
            let val = (self.0 >> 15usize) & 0x01;
            val != 0
        }
        #[doc = "Interrupt on setup packet"]
        #[inline(always)]
        pub fn set_ios(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 15usize)) | (((val as u32) & 0x01) << 15usize);
        }
        #[doc = "Maximum packet size"]
        #[inline(always)]
        pub const fn max_packet_size(&self) -> u16 {
            let val = (self.0 >> 16usize) & 0x07ff;
            val as u16
        }
        #[doc = "Maximum packet size"]
        #[inline(always)]
        pub fn set_max_packet_size(&mut self, val: u16) {
            self.0 = (self.0 & !(0x07ff << 16usize)) | (((val as u32) & 0x07ff) << 16usize);
        }
        #[doc = "Zero length termination"]
        #[inline(always)]
        pub const fn zero_length_termination(&self) -> bool {
            let val = (self.0 >> 29usize) & 0x01;
            val != 0
        }
        #[doc = "Zero length termination"]
        #[inline(always)]
        pub fn set_zero_length_termination(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 29usize)) | (((val as u32) & 0x01) << 29usize);
        }
        #[doc = "Isochronous mult"]
        #[inline(always)]
        pub const fn iso_mult(&self) -> u8 {
            let val = (self.0 >> 30usize) & 0x03;
            val as u8
        }
        #[doc = "Isochronous mult"]
        #[inline(always)]
        pub fn set_iso_mult(&mut self, val: u8) {
            self.0 = (self.0 & !(0x03 << 30usize)) | (((val as u32) & 0x03) << 30usize);
        }
    }
    impl Default for Cap {
        #[inline(always)]
        fn default() -> Cap {
            Cap(0)
        }
    }
    #[doc = "Current dtd address"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct CurDtd(pub u32);
    impl CurDtd {
        #[doc = "32-byte aligned address for current dtd, only bits 5-32 are valid"]
        #[inline(always)]
        pub const fn cur_dtd_addr(&self) -> u32 {
            let val = (self.0 >> 5usize) & 0x07ff_ffff;
            val as u32
        }
        #[doc = "32-byte aligned address for current dtd, only bits 5-32 are valid"]
        #[inline(always)]
        pub fn set_cur_dtd_addr(&mut self, val: u32) {
            self.0 = (self.0 & !(0x07ff_ffff << 5usize)) | (((val as u32) & 0x07ff_ffff) << 5usize);
        }
    }
    impl Default for CurDtd {
        #[inline(always)]
        fn default() -> CurDtd {
            CurDtd(0)
        }
    }
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct CurrentOffset(pub u32);
    impl CurrentOffset {
        #[doc = "Current offset in buffer"]
        #[inline(always)]
        pub const fn current_offset(&self) -> u16 {
            let val = (self.0 >> 0usize) & 0x0fff;
            val as u16
        }
        #[doc = "Current offset in buffer"]
        #[inline(always)]
        pub fn set_current_offset(&mut self, val: u16) {
            self.0 = (self.0 & !(0x0fff << 0usize)) | (((val as u32) & 0x0fff) << 0usize);
        }
    }
    impl Default for CurrentOffset {
        #[inline(always)]
        fn default() -> CurrentOffset {
            CurrentOffset(0)
        }
    }
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct ExpectedBytes(pub u32);
    impl ExpectedBytes {
        #[doc = "Number of bytes expected to transfer"]
        #[inline(always)]
        pub const fn expected_bytes(&self) -> u16 {
            let val = (self.0 >> 0usize) & 0xffff;
            val as u16
        }
        #[doc = "Number of bytes expected to transfer"]
        #[inline(always)]
        pub fn set_expected_bytes(&mut self, val: u16) {
            self.0 = (self.0 & !(0xffff << 0usize)) | (((val as u32) & 0xffff) << 0usize);
        }
    }
    impl Default for ExpectedBytes {
        #[inline(always)]
        fn default() -> ExpectedBytes {
            ExpectedBytes(0)
        }
    }
    #[doc = "Next dtd address and termination control"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct NextDtd(pub u32);
    impl NextDtd {
        #[doc = "Terminate bit, 1 represents current DTD is the last one"]
        #[inline(always)]
        pub const fn t(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "Terminate bit, 1 represents current DTD is the last one"]
        #[inline(always)]
        pub fn set_t(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u32) & 0x01) << 0usize);
        }
        #[doc = "32-byte aligned address for next dtd, only bits 5-32 are valid"]
        #[inline(always)]
        pub const fn next_dtd_addr(&self) -> u32 {
            let val = (self.0 >> 5usize) & 0x07ff_ffff;
            val as u32
        }
        #[doc = "32-byte aligned address for next dtd, only bits 5-32 are valid"]
        #[inline(always)]
        pub fn set_next_dtd_addr(&mut self, val: u32) {
            self.0 = (self.0 & !(0x07ff_ffff << 5usize)) | (((val as u32) & 0x07ff_ffff) << 5usize);
        }
    }
    impl Default for NextDtd {
        #[inline(always)]
        fn default() -> NextDtd {
            NextDtd(0)
        }
    }
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct QtdToken(pub u32);
    impl QtdToken {
        #[doc = "Status and control"]
        #[inline(always)]
        pub const fn status(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0xff;
            val as u8
        }
        #[doc = "Status and control"]
        #[inline(always)]
        pub fn set_status(&mut self, val: u8) {
            self.0 = (self.0 & !(0xff << 0usize)) | (((val as u32) & 0xff) << 0usize);
        }
        #[doc = "Transaction error"]
        #[inline(always)]
        pub const fn transaction_err(&self) -> bool {
            let val = (self.0 >> 3usize) & 0x01;
            val != 0
        }
        #[doc = "Transaction error"]
        #[inline(always)]
        pub fn set_transaction_err(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 3usize)) | (((val as u32) & 0x01) << 3usize);
        }
        #[doc = "Buffer error, underrun(IN) or overrun(OUT)"]
        #[inline(always)]
        pub const fn buffer_err(&self) -> bool {
            let val = (self.0 >> 5usize) & 0x01;
            val != 0
        }
        #[doc = "Buffer error, underrun(IN) or overrun(OUT)"]
        #[inline(always)]
        pub fn set_buffer_err(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 5usize)) | (((val as u32) & 0x01) << 5usize);
        }
        #[doc = "Whether current dtd is halted"]
        #[inline(always)]
        pub const fn halted(&self) -> bool {
            let val = (self.0 >> 6usize) & 0x01;
            val != 0
        }
        #[doc = "Whether current dtd is halted"]
        #[inline(always)]
        pub fn set_halted(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 6usize)) | (((val as u32) & 0x01) << 6usize);
        }
        #[doc = "Whether current dtd is active"]
        #[inline(always)]
        pub const fn active(&self) -> bool {
            let val = (self.0 >> 7usize) & 0x01;
            val != 0
        }
        #[doc = "Whether current dtd is active"]
        #[inline(always)]
        pub fn set_active(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 7usize)) | (((val as u32) & 0x01) << 7usize);
        }
        #[doc = "Multiplier"]
        #[inline(always)]
        pub const fn multo(&self) -> u8 {
            let val = (self.0 >> 10usize) & 0x03;
            val as u8
        }
        #[doc = "Multiplier"]
        #[inline(always)]
        pub fn set_multo(&mut self, val: u8) {
            self.0 = (self.0 & !(0x03 << 10usize)) | (((val as u32) & 0x03) << 10usize);
        }
        #[doc = "Current page"]
        #[inline(always)]
        pub const fn c_page(&self) -> u8 {
            let val = (self.0 >> 12usize) & 0x07;
            val as u8
        }
        #[doc = "Current page"]
        #[inline(always)]
        pub fn set_c_page(&mut self, val: u8) {
            self.0 = (self.0 & !(0x07 << 12usize)) | (((val as u32) & 0x07) << 12usize);
        }
        #[doc = "Interrupt on complete"]
        #[inline(always)]
        pub const fn ioc(&self) -> bool {
            let val = (self.0 >> 15usize) & 0x01;
            val != 0
        }
        #[doc = "Interrupt on complete"]
        #[inline(always)]
        pub fn set_ioc(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 15usize)) | (((val as u32) & 0x01) << 15usize);
        }
        #[doc = "Total bytes to transfer"]
        #[inline(always)]
        pub const fn total_bytes(&self) -> u16 {
            let val = (self.0 >> 16usize) & 0x7fff;
            val as u16
        }
        #[doc = "Total bytes to transfer"]
        #[inline(always)]
        pub fn set_total_bytes(&mut self, val: u16) {
            self.0 = (self.0 & !(0x7fff << 16usize)) | (((val as u32) & 0x7fff) << 16usize);
        }
    }
    impl Default for QtdToken {
        #[inline(always)]
        fn default() -> QtdToken {
            QtdToken(0)
        }
    }
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct SetupBuffer(pub u32);
    impl SetupBuffer {
        #[doc = "Buffer for setup packet"]
        #[inline(always)]
        pub const fn setup_buffer(&self) -> u32 {
            let val = (self.0 >> 0usize) & 0xffff_ffff;
            val as u32
        }
        #[doc = "Buffer for setup packet"]
        #[inline(always)]
        pub fn set_setup_buffer(&mut self, val: u32) {
            self.0 = (self.0 & !(0xffff_ffff << 0usize)) | (((val as u32) & 0xffff_ffff) << 0usize);
        }
    }
    impl Default for SetupBuffer {
        #[inline(always)]
        fn default() -> SetupBuffer {
            SetupBuffer(0)
        }
    }
}
