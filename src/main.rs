use std::{
    alloc::Layout, sync::{mpsc::channel, Arc}, thread
};

use tokio::sync::Semaphore;

fn main() {
    let (tx, rx) = channel();
    let tx = Arc::new(tx);
    let mut dev_path = std::env::args().nth(1).unwrap();
    dev_path.push('\0');

    let fd = unsafe {
        libc::open(
            dev_path.as_ptr() as *const i8,
            libc::O_DIRECT | libc::O_LARGEFILE | libc::O_RDWR,
        )
    };
    assert!(fd > 0, "{}", unsafe { *libc::__errno_location() });

    let iodepth = 16;
    let blksize = 256 << 10;
    let mut dev_size = 0;
    unsafe { ioctls::blkgetsize64(fd, &mut dev_size) };

    let layout = Layout::from_size_align(blksize, 4096).unwrap();
    let ptr = unsafe { std::alloc::alloc(layout) } as usize;
    println!("ptr: {:x}", ptr);

    let sem = Arc::new(Semaphore::new(iodepth));

    for i in 0..iodepth {
        let tx = tx.clone();
        let sem: Arc<Semaphore> = sem.clone();
        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let mut off = (i * blksize) as u64;
            while off < dev_size {
                rt.block_on(sem.acquire()).unwrap().forget();
                let ret = unsafe {
                    libc::pwrite(
                        fd,
                        ptr as *const libc::c_void,
                        blksize,
                        off as i64,
                    )
                };
                assert_eq!(ret, blksize as isize, "{}", unsafe {
                    *libc::__errno_location()
                });
                off += (blksize * iodepth) as u64;
                tx.send(off).unwrap();
            }
        });
    }

    drop(tx);

    while let Ok(off) = rx.recv() {
        let range = [off, blksize as u64];
        unsafe { ioctls::blkdiscard(fd, &range) };
	sem.add_permits(1);
    }
}
